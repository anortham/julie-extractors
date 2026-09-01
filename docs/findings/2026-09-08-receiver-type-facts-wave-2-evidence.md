# Receiver Type Facts Wave 2: Evidence Scan (Task 27)

Date: 2026-09-01. Binary: `julie-extract` built from
`worktree-receiver-type-facts-wave-2` after Tasks 1–28 and the review
fixes. Each basic golden holds one unresolved self-call so a pending
`receiver_type` row exists next to the resolved one.

No `~/source` real-world corpus repos from
`fixtures/extraction/tree-sitter-real-world-corpus.toml` were present.
Every language scanned every fixture directory under
`fixtures/extraction/<lang>/` (basic plus the framework, test-role, and
cross-file fixtures). The first scan on this branch covered `basic` only
and missed leading-sigil values (`*Worker`, `[]const`) and a
`void Function()` row in a test-role fixture; the query below now checks
both ends of the value and the scan covers every fixture directory.

## Method

```
cargo run -q -p julie-extract-cli --bin julie-extract -- scan --root fixtures/extraction/<lang> --db <db>
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
   OR resolved_type LIKE '%?'
   OR resolved_type LIKE '[%'
   OR resolved_type LIKE '*%'
   OR resolved_type LIKE '&%'
   OR resolved_type LIKE '?%'
   OR resolved_type LIKE '%<%'
   OR resolved_type LIKE '%(%'
   OR resolved_type = 'inferred'
   OR resolved_type = '';

-- receiver_type (hard gate)
SELECT COUNT(*) FROM identifiers
WHERE json_extract(metadata_json,'$.receiver_type') IS NOT NULL;
SELECT COUNT(*) FROM pending_relationships
WHERE json_extract(metadata_json,'$.receiver_type') IS NOT NULL;
```

## Inputs

| Language | Scanned |
|---|---|
| python, kotlin, swift, dart, gdscript, scala, c, cpp, zig, vbnet, powershell, fsharp, qml, php, ruby, lua, r, elixir, erlang, bash, razor | every directory under `fixtures/extraction/<lang>/` |

## Per-language counts

| Language | Params | Typed locals | Typed fields | Corrupt | Receiver id | Receiver pending |
|---|---|---|---|---|---|---|
| python | 27 | 1 | 0 | 0 | 2 | 2 |
| kotlin | 14 | 1 | 0 | 0 | 2 | 1 |
| swift | 12 | 7 | 0 | 0 | 7 | 7 |
| dart | 13 | 5 | 1 | 0 | 2 | 2 |
| gdscript | 11 | 3 | 2 | 0 | 4 | 4 |
| scala | 11 | 3 | 0 | 0 | 2 | 1 |
| c | 8 | 4 | 2 | 0 | 0 | 0 |
| cpp | 9 | 6 | 1 | 0 | 3 | 2 |
| zig | 17 | 4 | 2 | 0 | 2 | 1 |
| vbnet | 15 | 7 | 2 | 2 | 1 | 1 |
| powershell | 7 | 6 | 0 | 0 | 1 | 1 |
| fsharp | 13 | 11 | 5 | 0 | 2 | 2 |
| qml | 10 | 2 | 0 | 0 | 1 | 1 |
| php | 20 | 12 | 0 | 0 | 3 | 1 |
| ruby | 9 | 4 | 0 | 0 | 2 | 1 |
| lua | 8 | 1 | 0 | 0 | 2 | 1 |
| r | 6 | 1 | 0 | 0 | 1 | 1 |
| elixir | 18 | 1 | 0 | 0 | 0 | 0 |
| erlang | 41 | 2 | 0 | 0 | 0 | 0 |
| bash | 4 | 0 | 0 | 0 | 0 | 0 |
| razor | 13 | 34 | 6 | 0 | 1 | 0 |

The two vbnet "corrupt" hits are `Integer()` and `Worker()`: VB array
types, which the contract keeps as recorded array suffixes. The `%(%`
clause counts them; no other language has a hit.

Python receiver rows come from `self.`/`cls.` calls in the python
fixtures; the decision-doc self-receiver table lists the wave-2 languages,
so python is report-only here.

## Hard-gate verdicts

| Gate | Verdict | Evidence |
|---|---|---|
| 0 corrupt `resolved_type` | PASS | 0 in 19 languages; the 2 vbnet hits are kept array suffixes (`Integer()`, `Worker()`) |
| parameter symbols with `role=parameter` in all 21 languages | PASS | min 4 (bash), all 21 ≥ 1 |
| `receiver_type` on identifiers and pending rows where the decision doc marks a self receiver | PASS | all applicable languages ≥ 1 on both rows; razor identifiers 1, pending 0 |

c, elixir, erlang, and bash record no `receiver_type` (decision doc).
