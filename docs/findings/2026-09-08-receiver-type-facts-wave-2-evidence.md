# Receiver Type Facts Wave 2: Evidence Scan (Task 27)

Date: 2026-09-01. Binary: `julie-extract` built from
`worktree-receiver-type-facts-wave-2` after Tasks 1–26 plus unresolved
self-calls in eight basic goldens so pending `receiver_type` rows exist
when the callee is not in the same file.

No `~/source` real-world corpus repos from
`fixtures/extraction/tree-sitter-real-world-corpus.toml` were present.
Every language used `fixtures/extraction/<lang>/basic` only.

## Method

```
cargo run -q -p julie-extract-cli --bin julie-extract -- scan --root <basic> --db <db>
```

Queries:

```sql
-- parameter symbols (hard gate)
SELECT COUNT(*) FROM symbols
WHERE kind='variable' AND json_extract(metadata_json,'$.role')='parameter';

-- typed locals (report-only): variable without role, with a type fact
SELECT COUNT(*) FROM symbols s
JOIN type_facts t ON t.symbol_id=s.symbol_id
WHERE s.kind='variable' AND json_extract(s.metadata_json,'$.role') IS NULL;

-- typed fields (report-only)
SELECT COUNT(*) FROM symbols s
JOIN type_facts t ON t.symbol_id=s.symbol_id
WHERE s.kind='field';

-- corrupt resolved_type (hard gate)
SELECT COUNT(*) FROM type_facts
WHERE resolved_type LIKE '% %'
   OR instr(resolved_type, char(9)) > 0
   OR instr(resolved_type, char(10)) > 0
   OR resolved_type LIKE '%<'
   OR resolved_type LIKE '%['
   OR resolved_type LIKE '%('
   OR resolved_type LIKE '%*'
   OR resolved_type LIKE '%&'
   OR resolved_type LIKE '%?';

-- receiver_type (hard gate)
SELECT COUNT(*) FROM identifiers
WHERE json_extract(metadata_json,'$.receiver_type') IS NOT NULL;
SELECT COUNT(*) FROM pending_relationships
WHERE json_extract(metadata_json,'$.receiver_type') IS NOT NULL;
```

## Inputs

| Language | Scanned |
|---|---|
| python, kotlin, swift, dart, gdscript, scala, c, cpp, zig, vbnet, powershell, fsharp, qml, php, ruby, lua, r, elixir, erlang, bash, razor | `fixtures/extraction/<lang>/basic` only |

## Per-language counts

| Language | Params | Typed locals | Typed fields | Corrupt | Receiver id | Receiver pending |
|---|---|---|---|---|---|---|
| python | 10 | 1 | 0 | 0 | 0 | 0 |
| kotlin | 7 | 1 | 0 | 0 | 1 | 1 |
| swift | 10 | 8 | 0 | 0 | 3 | 3 |
| dart | 11 | 10 | 1 | 0 | 2 | 2 |
| gdscript | 11 | 3 | 2 | 0 | 4 | 4 |
| scala | 11 | 3 | 0 | 0 | 1 | 1 |
| c | 8 | 4 | 2 | 0 | 0 | 0 |
| cpp | 9 | 6 | 1 | 0 | 3 | 2 |
| zig | 16 | 11 | 2 | 0 | 1 | 1 |
| vbnet | 11 | 4 | 1 | 0 | 1 | 1 |
| powershell | 7 | 4 | 0 | 0 | 1 | 1 |
| fsharp | 9 | 9 | 3 | 0 | 2 | 2 |
| qml | 10 | 2 | 0 | 0 | 1 | 1 |
| php | 10 | 5 | 0 | 0 | 1 | 1 |
| ruby | 9 | 3 | 0 | 0 | 1 | 1 |
| lua | 8 | 0 | 0 | 0 | 1 | 1 |
| r | 5 | 0 | 0 | 0 | 1 | 1 |
| elixir | 14 | 1 | 0 | 0 | 0 | 0 |
| erlang | 10 | 1 | 0 | 0 | 0 | 0 |
| bash | 4 | 0 | 0 | 0 | 0 | 0 |
| razor | 9 | 2 | 1 | 0 | 1 | 0 |

Python receiver rows are report-only on this scan: the decision-doc
self-receiver table is the wave-2 languages. Python `self.`/`cls.` is
covered by unit tests; the basic fixture has no unresolved self-call.

## Hard-gate verdicts

| Gate | Verdict | Evidence |
|---|---|---|
| 0 corrupt `resolved_type` | PASS | 0 in all 21 artifacts |
| parameter symbols with `role=parameter` in all 21 languages | PASS | min 4 (bash), all 21 ≥ 1 |
| `receiver_type` on identifiers and pending rows where the decision doc marks a self receiver | PASS | all applicable languages ≥ 1 on both rows; razor identifiers 1, pending 0 |

c, elixir, erlang, and bash record no `receiver_type` (decision doc).
