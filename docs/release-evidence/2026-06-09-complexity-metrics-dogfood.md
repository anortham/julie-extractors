# Complexity Metrics Dogfood Evidence

Date: 2026-06-09

## Command

```bash
cargo run -p julie-extract-cli --bin julie-extract -- scan --root . --db target/complexity-metrics-dogfood/artifact.sqlite --force --json
```

Result: success.

Artifact path:

```text
target/complexity-metrics-dogfood/artifact.sqlite
```

Scan report summary:

- files scanned: 1086
- files changed: 1083
- unsupported files: 3
- failed files: 0
- `complexity_metrics` rows written: 6943

## Complexity Rows By Language And Scope

Query:

```sql
SELECT language, scope, COUNT(*)
FROM complexity_metrics
GROUP BY language, scope
ORDER BY language, scope;
```

Rows:

| Language | Scope | Count |
| --- | --- | ---: |
| `c` | `file` | 3 |
| `c` | `symbol` | 6 |
| `cpp` | `file` | 3 |
| `cpp` | `symbol` | 9 |
| `go` | `file` | 4 |
| `go` | `symbol` | 9 |
| `javascript` | `file` | 3 |
| `javascript` | `symbol` | 6 |
| `python` | `file` | 4 |
| `python` | `symbol` | 12 |
| `rust` | `file` | 794 |
| `rust` | `symbol` | 6080 |
| `typescript` | `file` | 3 |
| `typescript` | `symbol` | 7 |

This proves the first supported matrix emits both `file` and `symbol` scopes.
The counts are dogfood evidence, not release thresholds.
