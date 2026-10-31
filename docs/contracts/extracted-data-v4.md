# Extracted Data Contract v4

Extraction contract 4 pairs with [SQLite schema 8](sqlite-schema-v8.md).
Schemas 5, 6, and 7 remain historical pairings.

> **2026-08-18 retirement:** this product no longer writes workspace-global
> reference resolution. Identifier `target_symbol_id` is not stored on the
> artifact. code-kb computes resolution at query time. See
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

## Identifier receiver facts

Canonical extraction owns receiver metadata. The CLI serializes these facts
without inspecting source syntax. For a wholly named member chain such as
`a.b.run()`, `receiver` is `b` and `receiver_qualifier` is `a`. These keys
describe written qualification, not a resolved object or type.

AST evidence supports Unicode names, multiline access, comments between tokens,
and language-specific member syntax. Expression-rooted chains such as
`factory().b.run()` and `items[0].run()` do not acquire a guessed named receiver.
Existing language-specific receiver metadata remains authoritative as a pair;
an explicit null receiver suppresses both keys in the artifact.

`receiver_type` is independent type evidence from the language extractor.
Attaching receiver metadata does not change identifier spans, reference-site
identity, or relationship type facts. Embedded code is enriched against its own
syntax tree before coordinates are mapped to the host file.

## Variable access facts

`identifiers.metadata_json.access` describes the written use of a variable,
not data flow or a resolved binding. Its optional values are:

| Value | Written syntax |
| --- | --- |
| `write` | Assignment target, initialized declaration identifier, destructuring or pattern binding, increment/decrement, or an explicit `out`, `ref`, or `&mut` argument. |
| `read_write` | Compound assignment (including logical assignment), or a Swift `inout` argument. |
| `read` | A classified occurrence that is neither of the above. |
| absent | The extractor cannot classify this site. Absence never means `read`. |

For `a.b = value`, the member identifier `b` is a write and the receiver `a`
is a read. An indexed target reads its index and receiver; it does not assign
to the variables used to compute them. `member_access` identifiers may carry
the same key. `call` and `type_usage` identifiers never carry it.

A declaration with a symbol row does not gain a duplicate identifier solely
to record a write. Access facts enrich identifier occurrences, so symbol-only
bindings remain represented by their symbols. Dynamic syntax without a clear
access classification leaves the key absent. Embedded expressions are
classified against their guest-language syntax before host coordinates are
mapped. Existing metadata keys remain unchanged.

This additive optional key keeps extraction contract v4 and SQLite schema v8.
Per-language support and fixture evidence are recorded in
`fixtures/extraction/capabilities.json` and `docs/languages/`.

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
julie-extract capability claim. code-kb owns that policy.
