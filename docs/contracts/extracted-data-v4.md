# Extracted Data Contract v4

Extraction contract 4 pairs only with SQLite schema 5 and JSONL contract 4.

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

## Capability gaps

Gap status is the closed vocabulary `open | exception`. The certified reference-resolution
snapshot contains 103 open rows at resolution contract v6: 36 tier-3 receiver rows,
34 tier-2 import rows, and 33 tier-3 static-type rows (every language outside
`TIER3_STATIC_TYPE_LANGUAGES` = csharp, typescript, javascript). TypeScript and
JavaScript tier-2 coverage is closed. Unknown statuses are invalid.
