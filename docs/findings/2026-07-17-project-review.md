# Project Review Findings — 2026-07-17 (post-v2.14.0)

Full-project audit after the v2.14.0 release (parser runtime 0.26.11, C# 14 /
Swift / R grammar freshness, T-SQL and Razor quality). Scope: architecture,
contracts, operational risks, and language-capability parity. Prior finding
[`2026-06-09-project-review.md`](2026-06-09-project-review.md) was re-checked;
closed items are listed so they are not re-opened.

**Verdict:** Production-grade extraction product with strong contracts, honest
capability tracking, and green quality gates (`silent_cells=0`,
`quality_bar_debts=0`). Remaining risk is structural size debt, contract/doc
drift, operational edge cases, and ~50 real capability gaps (plus ~18 soft
test-role classifications on data languages). No critical correctness bugs
found in this pass.

---

## 1. Product shape (healthy)

```text
julie-extract (CLI)
  discovery → extraction → julie_extractors::extract_canonical
                        → julie_extract_artifact::writer (SQLite v4)
                        → julie_extract_artifact::jsonl (export v3)
```

- Clear product boundary; SQLite primary, JSONL secondary; no MCP/daemon/search
  creep ([`docs/architecture/product-boundary.md`](../architecture/product-boundary.md)).
- Panic isolation, blake3 incremental scans, spool-to-disk, data-loss guard
  remain strong.
- 36 languages; default suite gated (90s wall-clock); slow gates feature-gated.

Decisions live in [`docs/decisions/`](../decisions/) (no `docs/adr/`). Recorded
decisions were not re-litigated.

---

## 2. Closed since the 2026-06-09 review

Do not re-open these:

| Finding | Status |
| --- | --- |
| MSRV undeclared | Closed — `rust-version = "1.95"` in workspace `Cargo.toml` |
| CI cargo cache missing | Closed — `Swatinem/rust-cache` on default CI workflow |
| Per-call `Regex::new` hot paths | Largely closed — most sites use `LazyLock` |
| Thread-local parser cache | Evaluated and **explicitly deferred** in `TODO.md` (negligible measured win) |
| `TODO.md` open items 1–15 | All marked done |

---

## 3. Verified problems

### 3.1 Dead report codes (MEDIUM)

- `LockTimeout` and `SlowFileSkipped` appear in `ReportCode` / `ERROR_CODES` and
  contract docs, but production paths do not emit them.
- `>1 MiB` sources are hard-excluded in
  `crates/julie-extract-cli/src/discovery.rs` (`MAX_SOURCE_FILE_BYTES`) and
  counted as unsupported without a typed warning.
- Concurrent writers hit generic DB errors, not `lock_timeout`.

**Fix direction:** Emit `slow_file_skipped` (or document the hard-exclude as
intentional and remove the unused code from the public contract). Emit or
remove `lock_timeout` the same way — do not leave dead public codes.

### 3.2 Schema / contract doc drift (MEDIUM)

- Code: `SQLITE_SCHEMA_VERSION = 4`, extract contract v3, JSONL v3, report
  schema v3.
- Some contract examples (notably sample blocks in
  [`docs/contracts/reports.md`](../contracts/reports.md) and historical CLI
  tables) still show SQLite schema `3`.

**Fix direction:** One hygiene pass so every public example matches the code
source of truth.

### 3.3 Additive-only schema / metadata edge cases (MEDIUM)

- No migration engine: `CREATE TABLE IF NOT EXISTS` plus additive overlays.
- Older SQLite versions readable unless `--strict-schema`; newer always
  rejected; contract mismatch always exit 3.
- No-change write paths can skip `write_metadata`, leaving stale
  `sqlite_schema_version` until a mutating write.

### 3.4 Specialist CI without rust-cache (MEDIUM)

- Default [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) caches.
- [`.github/workflows/specialist-gates.yml`](../../.github/workflows/specialist-gates.yml)
  does not — slower nightlies / dispatch runs.

### 3.5 Large-tree memory (MEDIUM)

- Full-file `fs::read` plus rayon (`-j`); results spooled under temp dir.
- Strengths (spool chunking, 1 MiB cap) exist, but there is no per-worker byte
  budget for pathological trees of many near-cap files.

### 3.6 Grammar fork maintenance (MEDIUM, operational)

- Owned forks for C#, SQL, Razor (plus other Git pins). Freshness report exists;
  PowerShell / QML / Razor drift is report-only until deliberately migrated.

### 3.7 Residual low-severity items from June

- Dart `unwrap` / dead `program` recovery path (if still present).
- C# return-type substring fragility in type inference.
- AST walk depth guards still absent in many recursive visitors (panic catcher
  contains the blast).
- Visibility / doc-comment consistency weaker for Python / Dart / Go.

---

## 4. Structural debt (compounding)

| File | Approx. LOC | Risk |
| --- | --- | --- |
| `crates/julie-extract-cli/src/resolution.rs` | ~2715 | Workspace ref resolution complexity |
| `crates/julie-extract-cli/src/commands.rs` | ~2204 | Still large after prior splits |
| `crates/julie-extract-artifact/src/jsonl.rs` | ~1901 | Export path density |
| `crates/julie-extract-artifact/src/writer.rs` | ~1563 | Improved but still domain-mixed |

**Closed candidate:** `structural_fact_registry` split into
`base/structural_fact_registry/{mod,builtins,data,sql,framework,web,http_client}.rs`
(plan executed).

---

## 5. Language capability parity

### 5.1 Stats (at review time)

| Metric | Value |
| --- | --- |
| Languages | 36 |
| `kind_coverage` domains | 11 |
| `open_gaps` | ~68 (~39 test_detection, ~29 structural_facts) |
| Soft / misclassified test gaps on data langs | ~18 |
| Real implementation gaps | ~50 |
| Strict quality report | `silent_cells=0`, `quality_bar_debts=0` |

Domain-level GPL coverage is even. Imbalance is **depth** (HTTP frameworks,
test roles, DSL structural facts), not missing domains.

### 5.2 Highest-value feasible closes

Peers already prove the pattern or goldens already show the construct:

1. **java / python** — `test_container` + `test_lifecycle` (detector/goldens
   already see JUnit / unittest hooks).
2. **csharp / vbnet / razor** — `test_lifecycle` (shared detector knows
   NUnit/MSTest keys).
3. **php** — `symfony.route.v1` (Laravel routes already shipped).
4. **kotlin** — `ktor.route.v1` (Spring mapping + gin/echo/axum patterns).
5. **sql** — advanced DML / procedure structure.
6. **css** — additional at-rules (`@supports` / `@layer` / `@container`, …).
7. **json** — `$ref` / `$schema` semantics (YAML anchors/aliases peer model).
8. **vue** — style→CSS scan, `#slot` shorthand.
9. **qml / gdscript** — test container/lifecycle (goldens already have
   TestCase / GutTest).
10. **scala** — test lifecycle hooks.
11. **html** — media / landmarks / `data-*` details.

### 5.3 Hard gaps (not cheap parity)

- Rust actix/axum **cross-file** mount/prefix joins.
- Next.js signal-free pages; axum 0.7 param version sniff.
- Deferred HTTP clients (hyper/ureq, OkHttp, Symfony HttpClient, …).

### 5.4 Tracking-system gaps (meta)

1. `open_gaps` satisfy `silent_cells` — the gate stays green while debt remains.
2. Soft-NA inconsistency: css/regex mark test roles `not_applicable`; html /
   json / toml / yaml / markdown / sql keep them open (intentional per
   [`2026-07-09-test-detection-applicability-audit.md`](2026-07-09-test-detection-applicability-audit.md),
   but inflates the ledger).
3. `types` / `type_argument_usages` / `pending_relationships` sit outside
   `kind_coverage`.
4. `supported[]` is fixture samples, not a full inventory — mechanical
   cross-lang kind diffs are noisy.
5. `reference_resolution` is documentation-only, not quality-gated.

**Policy follow-up (same day):**
[`docs/decisions/2026-07-17-capability-ledger-policy.md`](../decisions/2026-07-17-capability-ledger-policy.md)
locks the soft-NA rule, keeps observed domains out of `kind_coverage`, and adds
an informational `open_gap_backlog` metric to the quality report.

**Registry split (completed):**
[`docs/plans/2026-07-17-structural-fact-registry-module-split.md`](../plans/2026-07-17-structural-fact-registry-module-split.md)
executed — `structural_fact_registry.rs` is now
`base/structural_fact_registry/` with per-family SPECS modules.

---

## 6. Recommended backlog

1. Contract hygiene — schema-v4 examples; resolve dead report codes; refresh
   testing-strategy guardrail text if stale.
2. Test-role parity — java / python / csharp(+vbnet/razor), then qml /
   gdscript / scala.
3. HTTP/framework parity — Symfony routes, Ktor routes.
4. DSL depth — SQL DML/procs, CSS at-rules, JSON schema refs, Vue style/slot,
   HTML media.
5. Capability ledger cleanup — soft-NA policy, types in `kind_coverage` or
   documented exclusion, gap-backlog metric separate from `silent_cells`.
6. Structural split of `structural_fact_registry` — separate plan after (3)/(4)
   if churn continues.
7. Ops polish — specialist CI cache; optional scan memory budget; oversized-file
   typed warning (overlaps with 1).

---

## 7. Strengths to preserve

- Per-file `catch_unwind` isolation.
- blake3 content-hash incremental scans.
- Single-transaction WAL writes with prepared-statement reuse.
- Spool-to-disk extraction bounding peak memory.
- Atomic temp-then-rename JSONL export.
- Data-loss guard on failed extractions.
- Strict language-data-quality report with fixture-backed capability claims.
- Named test tiers + 90s default wall-clock tripwire + slow-gate convention
  tests.
- Deterministic grammar-freshness report (network path outside default tiers).
