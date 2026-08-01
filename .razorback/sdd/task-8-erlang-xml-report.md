# Task 8 — Erlang real-world corpus gate — Report

**Status:** DONE (with one prominent finding — see "Corpus finding")
**Commit:** `890db84`
**Worktree:** `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
**Branch:** `erlang-xml-language-support` (HEAD at start `90e6b02`, clean; HEAD after `890db84`, clean)
**Toolchain:** `RUSTUP_TOOLCHAIN=1.97.1` on every cargo invocation

## Worktree state check

| When | Path | Branch | HEAD | Dirty |
|---|---|---|---|---|
| Start | `.worktrees/erlang-xml-language-support` | `erlang-xml-language-support` | `90e6b02` | clean |
| End | same | same | `890db84` | clean |

`git worktree list` at end:

```
/Users/murphy/source/julie-extractors                                            4bee2fe [main]
/Users/murphy/.config/razorback/worktrees/julie-extractors/csharp-locals-params  90542e0 [feature/csharp-locals-params]
/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support     890db84 [erlang-xml-language-support]
```

Two pre-existing stashes exist on the repo (`stash@{0}` "pre-v2.2.0-release-main-dirty-state",
`stash@{1}` "pre-sync duplicate release state after v2.1.2 publish"). Both predate this task; I did not
create, restore, or touch either. No `git stash` was used in this task.

## Corpus finding — READ THIS FIRST

`telemetry-1.3.0/src/telemetry.erl` and `telemetry-1.3.0/src/telemetry.hrl` **do not parse clean**. Both
diagnose the same construct.

`telemetry.hrl:7` / `:9`:

```erlang
-define(WITH_STACKTRACE(T, R, S), T:R:S ->).
-define(WITH_STACKTRACE(T, R, S), T:R -> S = erlang:get_stacktrace(),).
```

The macro body is a *partial `catch` clause head*, not an expression. It is only valid Erlang after
preprocessor expansion. `telemetry.erl:169` and `:344` then use it as a clause head:

```erlang
catch
    ?WITH_STACKTRACE(Class, Reason, Stacktrace)
        detach(HandlerId),
        ...
```

tree-sitter has no preprocessor, so the grammar cannot recover. Result:

- `telemetry.hrl` — **2** diagnostics, one per `-define` body. All 13 symbols still extract.
- `telemetry.erl` — **45** diagnostics. The error regions are `169-184` (first call site) and `313-416`
  (second call site at `:344` plus the statements it swallows).

**Coverage cost:** `telemetry.erl` declares 8 exports. Four extract (`attach/4`, `attach_many/4`,
`detach/1`, `execute/3`); four do **not** — `list_handlers/1`, `execute/2`, `span/3`, `report_cb/1` — all
declared after line 184.

### Why this is not BLOCKED

- No file *fails*: `files_failed = 0`, `files_unsupported = 0`, every file's report status is `indexed`.
- The three spot-assertions the task named (`execute`, `attach`, `detach` as Public functions) all hold.
- The cause is an inherent tree-sitter limitation (no macro expansion), not a defect in this branch's
  extractor. It is the Erlang analogue of macro-heavy C/C++, and Erlang's own tooling needs `epp` here.
- The task spec explicitly permits this shape: "if any file legitimately produces diagnostics, the
  baseline records the exact count per file with a comment naming the construct."

### Root-cause proof (three minimal probes)

I isolated the cause rather than assuming it. Each probe is a 2-export module scanned with the real CLI:

| Probe | Construct | Symbols extracted | Diagnostics |
|---|---|---|---|
| A | `?DOC("""…""")` before a function | `p`, `f`, `g` — all | 0 |
| B | `?WITH_STACKTRACE(C, R, S)` as a `catch` clause head | `p`, `f` — **`g` lost** | 5 |
| C | bare OTP-27 triple-quoted string in a body | `p`, `f`, `g` — all | 0 |

So the OTP 27 `?DOC`/`?MODULEDOC`/triple-quoted-string constructs are fine on the pinned grammar; only
the macro-as-clause-head breaks, and it cascades past the enclosing function. That matches telemetry.erl
exactly.

**Recommendation (out of scope here, no production code changed):** if the branch wants those four
exports, it needs ERROR-node recovery that resumes top-level `fun_decl` scanning after an unparseable
form. That is an extractor change, not a fixture change. The gate's exact baseline will turn red the
moment such a change lands, forcing an explicit baseline update — which is the point of the gate.

## What was built

### 1. Vendored corpus — `fixtures/real-world/erlang/`

The issue's exact three packages, downloaded once from `repo.hex.pm`, outer `.tar` opened, then
`contents.tar.gz` extracted; only `src/**/*.erl`, `src/**/*.hrl`, `include/**/*.hrl`, and `LICENSE*`
kept. No package had an `include/` directory. `telemetry`'s `NOTICE` is also vendored (Apache-2.0 §4(d)
requires redistributing it).

| Package | Version | Tarball | License | Vendored `.erl`/`.hrl` |
|---|---|---|---|---|
| `telemetry` | 1.3.0 | `https://repo.hex.pm/tarballs/telemetry-1.3.0.tar` | Apache-2.0 | 6 |
| `certifi` | 2.15.0 | `https://repo.hex.pm/tarballs/certifi-2.15.0.tar` | BSD-3-Clause | 2 |
| `unicode_util_compat` | 0.7.1 | `https://repo.hex.pm/tarballs/unicode_util_compat-0.7.1.tar` | Apache-2.0 | 2 |

hex.pm outer-tarball `CHECKSUM` values recorded in the fixture `README.md`. Deliberately **excluded**:
`certifi`'s `test/certifi_tests.erl` (not under `src/`, per the spec), all `.app.src`, `rebar.config`,
`mix.exs`, `priv/cacerts.pem`, and package `README`/`CHANGELOG` files.

`CHECKSUMS.sha256` records SHA-256 of all 14 vendored files in `shasum -a 256` format (`<hex>  <path>`,
two spaces), verifiable with `shasum -a 256 -c CHECKSUMS.sha256` from the fixture directory. The
gate's third test re-verifies it in-process with the crate's existing `sha2` dependency, and also asserts
that no on-disk fixture file is missing from the manifest — so silently adding a corpus file fails.

### 2. The gate — `crates/julie-extract-cli/tests/erlang_corpus.rs`

`#![cfg(feature = "test-real-world")]`, three tests, ~3.3s wall total.

`ErlangCorpusScan::run()` copies **only** the `.erl`/`.hrl` files into a `TempDir` scan root preserving
relative paths, then invokes the real binary via `CARGO_BIN_EXE_julie-extract`:

```
julie-extract scan --root <temp>/repo --db <temp>/artifact.sqlite --json
```

The filtered copy is deliberate: `LICENSE`, `NOTICE`, `README.md`, and `CHECKSUMS.sha256` would otherwise
land in the scan root and be counted as `files_unsupported` / scanned as markdown, which would make a
`0 unsupported` assertion impossible and put non-Erlang rows in an Erlang baseline. It also guards
against fixture drift: the copy asserts the discovered source count equals `BASELINE.len()` before
scanning.

**Committed baseline (exact, no thresholds):**

Report-level (`counts`):

| Field | Value |
|---|---|
| `files_scanned` | 10 |
| `files_changed` | 10 |
| `files_unsupported` | 0 |
| `files_failed` | 0 |
| `file_rows_truncated` | `false` |
| `profile.languages` keys | exactly `["erlang"]` |
| `profile.languages.erlang.files` | 10 |
| `profile.languages.erlang.failed_files` | 0 |

Per file (`counts.file_rows[]`, each also asserted `language = erlang`, `status = indexed`):

| Path | symbols | parse_diagnostics |
|---|---|---|
| `certifi-2.15.0/src/certifi.erl` | 3 | 0 |
| `certifi-2.15.0/src/certifi_pt.erl` | 5 | 0 |
| `telemetry-1.3.0/src/telemetry.erl` | 16 | **45** |
| `telemetry-1.3.0/src/telemetry.hrl` | 13 | **2** |
| `telemetry-1.3.0/src/telemetry_app.erl` | 3 | 0 |
| `telemetry-1.3.0/src/telemetry_handler_table.erl` | 15 | 0 |
| `telemetry-1.3.0/src/telemetry_sup.erl` | 4 | 0 |
| `telemetry-1.3.0/src/telemetry_test.erl` | 3 | 0 |
| `unicode_util_compat-0.7.1/src/string_compat.erl` | 152 | 0 |
| `unicode_util_compat-0.7.1/src/unicode_util_compat.erl` | 58 | 0 |

The two nonzero diagnostic counts carry an inline doc comment on `BASELINE` naming the construct
(`?WITH_STACKTRACE`), the line numbers, and the exports it costs.

**Plausibility hand-review** (done before committing the numbers, as required):

- `telemetry.erl` 16 = 1 module + 11 `-type` + 4 functions. Verified symbol-by-symbol against the
  artifact. "Dozens, not 0 or 10,000" holds; the shortfall vs. 8 exports is the documented macro issue.
- `telemetry.hrl` 13 = 1 record + 4 record fields + 8 `-define` macros. Matches the file by hand count.
- `string_compat.erl` 152 symbols from 79 KB — plausible density for a wide utility module.
- `unicode_util_compat.erl` 58 symbols from 615 KB — this is a generated Unicode table module: few
  functions, enormous multi-clause bodies collapsed by the multi-clause rule from Task 2. Plausible.
- `certifi.erl` 3 / `certifi_pt.erl` 5 / `telemetry_app.erl` 3 / `telemetry_sup.erl` 4 /
  `telemetry_test.erl` 3 — all tiny modules; counts match a manual read.
- File size check: `unicode_util_compat.erl` at 615,460 bytes is under `MAX_SOURCE_FILE_BYTES`
  (1,048,576), so the Task 5 oversized-skip policy does not fire and the file is genuinely parsed.

**Spot-assertions (`telemetry_module_exposes_its_module_exports_and_behaviour_edges`)**, read from the
artifact DB with `rusqlite`:

- `telemetry` module symbol present, `kind = module`, `visibility = public`.
- `execute`, `attach`, `detach` each present as `kind = function`, `visibility = public`.
- `-behaviour` pending edges: `telemetry.erl` declares **none**, so per the spec's "for any behaviour it
  declares" the assertion covers the three modules in the package that do declare one —
  `telemetry_app.erl → application`, `telemetry_handler_table.erl → gen_server`,
  `telemetry_sup.erl → supervisor`. Each asserted as exactly one
  `pending_relationships` row with `kind = 'implements'` and the matching `target_display_name`.

**Scan wall-time (report-only, NOT asserted):** **3.32–3.34 s** for 10 files / 720,059 bytes, debug
profile, printed by the gate under `--nocapture`:

```
erlang corpus scan: 10 files, 720059 bytes, 3.34s wall
```

### 3. Feature declaration — `crates/julie-extract-cli/Cargo.toml`

```toml
test-real-world = []
```

with a comment pointing at the harness and the exact invocation, matching the existing `test-perf`
comment style directly above it.

## Invocation

```
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-real-world \
  --test erlang_corpus -- --nocapture
```

Also documented in the file's module doc comment, in the `Cargo.toml` feature comment, and in
`fixtures/real-world/erlang/README.md`.

## xtask decision — no change made

I did **not** touch `xtask/src/test_tiers.rs`. Findings, so the lead can decide for Task 10:

- The CLI crate's established gating convention is a plain feature-gated test target, not an xtask tier:
  `test-perf` + `#![cfg(feature = "test-perf")]` on `tests/resolution_perf.rs`, with
  `tests/perf_gate_convention.rs` guarding it. That is the precedent I followed, and it is the smallest
  change that satisfies the acceptance criterion.
- There *is* an xtask `real-world` / `real-world-release` tier, but it currently runs only
  `julie-extractors` lib tests. Adding the CLI gate to it would require editing **both**
  `xtask/src/test_tiers.rs` and `xtask/tests/test_tiers.rs` — the latter
  (`test_real_world_tier_selects_every_real_fixture_gate`, `xtask/tests/test_tiers.rs:241`) asserts the
  tier's exact command list and would fail otherwise. `xtask/tests/test_tiers.rs` is **outside my file
  ownership**, so I stopped rather than edit it.
- **Recommendation for Task 10:** wire `cargo test -p julie-extract-cli --features test-real-world
  --test erlang_corpus` into `real_world_release_plan()` and update the matching xtask assertion, so the
  tier lives up to its "every real fixture gate" name.

I also chose the feature name `test-real-world` (not a new `test-corpus`) to match the existing
`julie-extractors` real-world feature, so that future wiring is a one-liner.

## Auto-discovery check

`fixtures/real-world/erlang/` is **not** picked up by any other gate:

- `grep -rn "fixtures/real-world" --include='*.rs' crates/ xtask/` returns only my new file and
  `crates/julie-extractors/src/tests/json/mod.rs:888` (the hard-coded `real-world/json/memories.jsonl`
  path). No globbing, no directory walk over `fixtures/real-world/`.
- `fixtures/extraction/capabilities.json` is **untouched** — the golden and capability-matrix harnesses
  discover fixtures from its rows, so the new directory is invisible to them.
- The two erlang `kind_coverage` gaps that name Task 8 (literals, `behaviour_declaration` structural
  facts) were **not** touched, per contract. Task 10 normalizes those pointers.

## Miller calls (API-shape evidence), workspace_id `julie-extractors-91c17adbdab9`

| Call | What it proved |
|---|---|
| `context(query="how CLI integration tests run scans against temp workspaces", token_budget=2400)` | The scan-invocation harness shape: `scan_fixture` at `crates/julie-extract-cli/tests/resolution_contract.rs:49` — `TempDir::new()` → `copy_dir` fixture into `<temp>/repo` → `julie_extract(&["scan","--root",…,"--db",…,"--json"])` → assert `status.code() == Some(0)`. Also surfaced the helper set (`fixture_base:40`, `copy_dir:74`, `path_str:94`) and `ScanExtractionProfile` at `commands.rs:1068`. My `ErlangCorpusScan::run()` follows this shape exactly, substituting a filtered copy for `copy_dir`. |
| `search(query="test-perf feature gated test target tier", mode=source)` | Returned `no_text_hits` / `expected_empty`. Real result, not a miss: the gating convention is spelled out in prose and in `Cargo.toml`, not as that literal phrase. I fell back to reading `crates/julie-extract-cli/tests/perf_gate_convention.rs` and `xtask/src/test_tiers.rs` directly, which is where the convention actually lives. |

Miller reported `freshness: unconfirmed_lock_busy` on both calls (another indexer holds the lock). Every
pivot it returned was confirmed against the real file before I relied on it.

Report-schema field names came from the artifact crate itself rather than from Miller or memory:
`crates/julie-extract-artifact/src/reports.rs:191` `ReportCounts` (`files_scanned`, `files_changed`,
`files_unchanged`, `files_unsupported`, `files_deleted`, `files_failed`, `file_rows_truncated`,
`file_rows`), `:205` `ReportFileRows` (`path`, `language`, `status`, `total_rows`, `rows`), `:214`
`RowDomainCounts` (`symbols`, `parse_diagnostics`, …), and `:179` `ReportLanguageProfile` (`files`,
`failed_files`, …). Path separators are normalized to `/` by
`crates/julie-extract-cli/src/discovery.rs:251` (`.replace(std::path::MAIN_SEPARATOR, "/")`), so the
baseline's forward-slash paths are correct on Windows too.

## Verification

| Check | Command | Result |
|---|---|---|
| Gate passes (assigned) | `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus -- --nocapture` | **3 passed, 0 failed** in 3.33s |
| Default CLI suite (assigned, negative proof) | `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli` | **279 passed, 0 failed**; `tests/erlang_corpus.rs` reports `running 0 tests` — same shape as the existing `tests/resolution_perf.rs` gate |
| Workspace default tier | `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default` | exit 0, 23 `test result: ok` lines, no failures |
| Formatting | `cargo fmt --all -- --check` | clean (reformatted once, then clean) |
| Lints, gate enabled | `cargo clippy -p julie-extract-cli --features test-real-world --all-targets` | 0 warnings |
| Lints, default | `cargo clippy -p julie-extract-cli --all-targets` | 0 warnings |
| Checksums | `shasum -a 256 -c CHECKSUMS.sha256` (and the in-gate equivalent) | all 14 files OK |

**Default-suite wall time, before vs after** (warm cache, two runs each; measured by removing and
restoring the new test file in place — no `git stash`):

| | run 1 | run 2 |
|---|---|---|
| Before (gate file absent) | 5.33s | 5.29s |
| After (gate file present) | 5.29s | 5.31s |

Unchanged within noise. The new target adds a compile unit but zero tests.

## Self-review

- **Baseline is exact, not thresholded.** Every asserted number is an equality. No `>=`, no ranges.
- **The gate is a real scan.** It spawns the actual `julie-extract` binary through
  `CARGO_BIN_EXE_julie-extract`, not an in-process extractor call, so it exercises discovery, the spool,
  the writer, and the report path end to end.
- **No production code changed.** The diff is fixtures + one test file + one `Cargo.toml` feature line.
- **File ownership respected.** `xtask/src/test_tiers.rs` untouched (reason documented above);
  `capabilities.json` untouched; `perf_gate_convention.rs` untouched (a mirrored convention guard for
  this gate would be a good addition but that file is not mine to edit — noted for Task 10).
- **Failure modes the gate actually catches:** an extractor regression (symbol counts move), a grammar
  upgrade that fixes or worsens macro recovery (diagnostic counts move), fixture tampering or drift
  (checksums, file-count assertion), a language-detection regression (`profile.languages` keys), and a
  scan-path regression (nonzero `files_failed`/`files_unsupported`).
- **Known soft spot:** the symbol counts are sensitive to any deliberate extraction change (e.g. Task 9's
  structural facts, or the two open erlang `kind_coverage` gaps). That is intended — the gate should
  force an explicit, reviewed baseline update rather than drift silently. Whoever lands those changes
  updates `BASELINE` and says why.
- **Not done, deliberately:** no `-behaviour` structural-fact or literal assertions, because those two
  capability gaps are still open and are Task 10's to normalize.

---

## Fix round 1 — ERROR-node recovery

**Status:** BLOCKED — design decision required. **No commit; no source changed.** HEAD stays `890db84`,
worktree clean apart from this report file.

The requested change is **provably a no-op** on both the corpus and the existing broken fixture. I did not
implement it, because committing dead recovery code plus an unchanged baseline would look like a fix and
would not be one. I did find a mechanism that *does* work, validated it end to end, and it is described
below — but it is a materially different design from the one specified, so the call is the lead's.

## Why the specified fix cannot work

The review's premise was: *"the symbol walk never enters ERROR subtrees, so top-level `fun_decl` forms
AFTER the broken region (but inside the ERROR node) are lost."*

Three facts, each measured directly against the pinned `tree-sitter-erlang` 0.20.0 tree for
`telemetry.erl`:

**1. The walk already IS inside the top-level ERROR — the root node is one.**

```
root kind: ERROR
```

The whole file parses to a root `ERROR` node spanning lines 1–423. `named_children(&root)` therefore
already returns the ERROR's children. There is no unentered top-level ERROR subtree.

**2. Every nested ERROR contains zero recoverable declaration forms.**

Scanning each nested top-level `ERROR` child for the declaration kinds the extractor handles
(`fun_decl`, `module_attribute`, `record_decl`, `pp_define`, `type_alias`, `opaque`, `callback`,
`export_attribute`, `export_type_attribute`, `compile_options_attribute`, `wild_attribute`, `spec`,
`pp_include`, `pp_include_lib`), recursing through nested ERRORs — exactly what the proposed helper would
add:

```
ERROR [169..184] -> recoverable decl kinds: []
ERROR [314..315] -> recoverable decl kinds: []
ERROR [344..350] -> recoverable decl kinds: []
ERROR [385..385] -> recoverable decl kinds: []
ERROR [412..412] -> recoverable decl kinds: []
ERROR [413..413] -> recoverable decl kinds: []
ERROR [413..416] -> recoverable decl kinds: []
```

**3. The four missing exports do not exist as `fun_decl` nodes anywhere in the tree.**

Exhaustive scan for `fun_decl` at *any* depth:

```
total fun_decl = 9
fun_decl as DIRECT named children of root = 9
  line 15  <none>   (?MODULEDOC macro form)
  line 52  <none>   (?DOC macro form)
  line 82  attach
  line 85  <none>   (?DOC)
  line 112 attach_many
  line 127 <none>   (?DOC)
  line 133 detach
  line 136 <none>   (?DOC)
  line 156 execute
```

All 9 are already reached; 4 become symbols and 5 are `?DOC`/`?MODULEDOC` macro forms with no
`function_clause`, correctly dropped. There is no tenth `fun_decl` hiding anywhere.

`tree-sitter-erlang` does not preserve the following forms as damaged-but-intact declarations — it
**reinterprets** them as expression nodes belonging to the broken construct:

| Lost export | Source line | What it parsed as |
|---|---|---|
| `span/3` | 315 | `ERROR [314..315]` head, then loose `match_expr` / `call` / `cr_clause` siblings |
| `execute/2` | 360 | swallowed inside `catch_clause [351..385]` |
| `list_handlers/1` | 371 | swallowed inside `catch_clause [351..385]` |
| `report_cb/1` | 416 | inside `ERROR [413..416]` / `tuple [417..422]` |

The private helpers land the same way — `assert_event_names/1`, `assert_event_prefix/1`,
`assert_event_name/1` each parse as a top-level `catch_clause`, because `Pattern -> Body` is
structurally what a `catch` clause looks like.

**4. Same no-op on the existing `parse_errors.rs` fixture.** `audit/1` (the function after the broken
form) parses as `call` inside a nested `ERROR`, not as a `fun_decl`:

```
ERROR [7..10] "broken(( ->\n\naudit(A) ->\n    A."
  atom [7..7] "broken"
  ERROR [7..9] "->\n\naudit(A) ->"
    call [9..9] "audit(A)"
  var [10..10] "A"
```

So the specified change would add a shared ERROR-descending iterator that never yields a single extra
symbol, on either the corpus or the one fixture we have that exercises parse errors. The BASELINE would
not move, and the required "all 8 exports" spot-assertion would still fail. I would also have no test
that proves the new code path ever fires — the negative-control test would pass vacuously.

### A heuristic I considered and rejected

A top-level `catch_clause` cannot legitimately occur in Erlang, so treating one as a misparsed function
clause is a narrowly defensible rule. I checked what it would actually buy: it recovers only the private
helpers (`assert_event_names/1`, `assert_event_prefix/1`, `assert_event_name/1`) and **none** of the four
required exports, because `execute/2` and `list_handlers/1` are nested *inside* `catch_clause [351..385]`
rather than being top-level catch clauses themselves. Not worth the false-positive surface.

## What does work — offset-preserving form isolation

The damage is an unbounded cascade at **file** scope. At **form** scope it is contained. Re-parsing an
individual form in isolation recovers it, including forms that themselves contain `?WITH_STACKTRACE`:

| Isolated form | root | has_error | `fun_decl` |
|---|---|---|---|
| `execute/3` clause 2 (lines 160–184, contains `?WITH_STACKTRACE`) | `source_file` | true | **1** |
| `span/3` (315–352, contains `?WITH_STACKTRACE`) | `source_file` | true | **1** |
| `execute/2` (360–369) | `source_file` | **false** | 2 |
| `list_handlers/1` (371–382) | `source_file` | **false** | 1 |
| `report_cb/1` (416–423) | `source_file` | **false** | 1 |

Even the forms that still error keep their enclosing `fun_decl` intact, because the error no longer
swallows the rest of the file.

The byte-offset problem has a clean solution I verified rather than assumed. Build a re-parse buffer the
**same length** as the original file, with every byte outside the target form replaced by a space and
every newline preserved. Node byte ranges and line/column positions then match the original file exactly,
so `BaseExtractor::get_node_text` and `create_symbol` work unchanged against the real content buffer:

```
execute/2:       fun_decl heads recovered at original lines [(360, "execute"), (363, "<none>")]
list_handlers/1: fun_decl heads recovered at original lines [(371, "list_handlers")]
report_cb/1:     fun_decl heads recovered at original lines [(416, "report_cb")]
span/3:          fun_decl heads recovered at original lines [(315, "span")]
```

All four names were read out of the **original** content by the recovered nodes' byte ranges — that is the
proof that offsets align. All four missing exports recover, at their true line numbers.

### Why this is a design decision, not something I should just do

- It is a **new mechanism**, not the specified "scan ERROR children" refactor, and the round explicitly
  said not to build new recovery logic beyond the shared iterator.
- It needs an **Erlang form splitter** — find `.` tokens that terminate a top-level form — which must
  handle `"..."`, `"""..."""` (this corpus is full of them), `'...'`, `%` comments, `$.` character
  literals, and floats. Roughly 80 lines with its own correctness surface, and getting it wrong
  silently mangles good files, not just broken ones.
- It changes **parse cost** on shared extraction code: one extra parse per recovered form. On
  `unicode_util_compat.erl` (615 KB) an unbounded version is O(n²). It needs a trigger (`root.has_error()`
  only), a region bound (first error → EOF), and a cap on re-parses, all of which are policy choices.
- It affects `julie-extractors` core for **every** Erlang file, not just the corpus, so it wants its own
  design note and language-parity thinking rather than being smuggled into a corpus-gate fix round.

**Estimate:** one focused session for the splitter + isolation + wiring into `extract_symbols` and the
pre-scan, plus tests; the identifier/relationship/type walks would need the same treatment or an explicit
decision to leave them on the primary tree only.

## What I need from the lead

Pick one:

1. **Implement offset-preserving form isolation** in this branch (I have the mechanism validated; give me
   the go-ahead and the parse-budget policy — trigger, region bound, re-parse cap).
2. **Defer it** to its own task with a design note, and accept the current baseline for Task 8, with the
   `telemetry.erl` shortfall recorded as a known `tree-sitter-erlang` limitation. Gate 2's "exported
   functions present in symbols" would then need rewording to "exported functions present for forms the
   grammar can parse", or the acceptance corpus loosened.
3. **Something else** — e.g. pin a newer/forked grammar with better form-level error recovery, which I have
   not evaluated.

I did **not** weaken the gate, relax the baseline, or edit the acceptance criterion to make the shortfall
disappear.

## Verification run this round

None of the assigned commands were run: no source changed, so there was nothing to verify red or green.
The committed state at `890db84` is the one already verified in the main report (corpus gate 3 passed,
default CLI suite 279 passed, `cargo xtask test default` exit 0, fmt and clippy clean).

Evidence above was produced with a throwaway probe crate in `/tmp/erldump` pinned to the same grammar
versions as the workspace (`tree-sitter = "=0.26.11"`, `tree-sitter-erlang = "=0.20.0"`). Nothing was
written inside the repository.

### Addendum — sharpened after the lead's status check

The heading above was originally written as `#` instead of `##`, which is why a `## Fix round 1` grep
missed it. Fixed. The status is unchanged: **BLOCKED on a design decision, no commit.**

Since reporting, I tested a mechanism *smaller* than the form-splitter I first proposed, to try to shrink
the blocker. It nearly works, and the residue is instructive.

**Iterative resync, no form splitter.** Parse normally; while the tree has an error, blank everything
before a resume point (preserving newlines, so byte offsets and line numbers stay identical to the
original file) and re-parse, merging newly recovered declarations. tree-sitter re-synchronises on its own
— no `.`-tokenizer, no string/comment handling needed.

Resuming at the line after each error's end:

```
pass 0: cut@line 1   root=ERROR       fresh: attach@82, attach_many@112, detach@133, execute@156
pass 1: cut@line 185 root=source_file fresh: span@315, list_handlers@371, assert_event_names@384,
                                             assert_event_names@386, assert_event_prefix@390,
                                             assert_event_prefix@397, assert_event_name@401,
                                             assert_event_name@408, merge_ctx@412, merge_ctx@413,
                                             report_cb@416
pass 2..5: no new declarations
pass 6: cut@line 371 root=source_file has_error=false  -> converged
```

**Result: 7 of the 8 exports recover** — `attach/4`, `attach_many/4`, `detach/1`, `execute/3`, `span/3`,
`list_handlers/1`, `report_cb/1` — plus all the private helpers. `report_cb/1` even comes back with the
right line. One export still resists: **`execute/2` at line 360.**

Why it resists, precisely:

```
354  ?DOC("""
355  Same as [`execute(EventName, Measurements, #{})`](`execute/3`).
356  """).
357  -spec execute(EventName, Measurements) -> ok when
358        EventName :: event_name(),
359        Measurements :: event_measurements() | event_value().
360  execute(EventName, Measurements) ->
361      execute(EventName, Measurements, #{}).
```

`execute/2` sits between two error regions. A resume point that lands mid-`-spec` (line 357–359) produces
an error that *ends on line 360*, so "resume at the line after the error end" jumps to 361 and eats the
function head. Line-aligning the cut does not help — it is the alignment that skips the form.

The fix for that last one is to stop resuming at error ends and instead resume at **top-level form-start
candidates** (a line beginning at column 0 with `-`, `?`, or a lowercase atom), advancing one candidate at
a time. That reaches line 357 or 360 and recovers `execute/2`. I stopped there rather than build it.

### Why I stopped instead of shipping the 7/8 version

- 7/8 does not satisfy gate 2. Committing it would move the baseline and still fail the acceptance
  criterion — the same "looks like a fix, isn't" problem as the no-op.
- The candidate-resume refinement turns this into a **bounded re-parse resync engine**, not the ~20-line
  shared-iterator refactor the round specified. It needs a re-parse cap, a resume-candidate rule, and a
  decision on whether the identifier/relationship/type walks run against recovered trees too.
- It resumes parsing at arbitrary offsets, so it can synthesise symbols from source that is not actually a
  declaration — e.g. column-0 lines *inside* a triple-quoted doc string, which this corpus is full of.
  That risk needs its own negative-control tests before it goes anywhere near shared extraction.
- It touches `julie-extractors` core for every Erlang file. Given the repo's language-parity rule — "a
  feature that silently covers only a subset but looks authoritative is a bug" — this wants a design note,
  not a fix-round improvisation.

**Cost estimate, unchanged:** one focused session. The mechanism is validated; what remains is the resume
rule, the cost bound, and the negative-control tests.

**I can start immediately on a go-ahead.** What I need is a yes to the resync approach plus the parse
budget (my proposed defaults: only run when `root.has_error()`, cap at 32 re-parses per file, resume only
at column-0 form-start candidates). Absent that, Task 8 stands at `890db84` with the shortfall documented
and gate 2 needing either the fix above or a rewording.

---

## Fix round 2 — bounded resync recovery

**Status:** DONE. **Commit:** `4cfe193` on `erlang-xml-language-support` (parent `f3c2f52`).
**8/8 confirmed:** `telemetry.erl` now yields every declared export as a Public function symbol.

### What shipped

`crates/julie-extractors/src/erlang/recovery.rs` (new) plus wiring in `mod.rs`, and signature changes
in `identifiers.rs` / `relationships.rs` so every walk consumes one declaration list.

**Algorithm.** Parse normally. If the root has an error, re-parse from successive top-level form starts:
blank every byte before the resume point to a space, keep newlines. The re-parsed buffer is byte-for-byte
the same length with the same line breaks, so recovered nodes carry offsets and line/column positions
valid against the *original* content — `BaseExtractor::get_node_text` and `create_symbol` need no
rebasing. tree-sitter re-synchronises by itself, which is why this needs no Erlang form tokenizer and no
handling for `"""`, `'…'`, `%`, `$.`, or floats in a splitter.

**Budget (the approved defaults, all implemented).**

| Control | Implementation |
|---|---|
| Trigger | `recover` returns `Vec::new()` before constructing a parser unless `primary.root_node().has_error()` |
| Cap | `MAX_RECOVERY_PARSES = 32`, a `for _ in 0..MAX` loop |
| Resume points | column-0 lines starting `-`, `?`, `'`, or `a..z`, minus any offset inside a `string` or `comment` node |
| Advance | each pass takes the first resume point strictly after the current tree's first error, never below the previous index; stops early when a re-parse is clean |
| Offsets | `blank_before` preserves byte length and newlines |

**Merging.** `merge_declarations` returns the primary tree's top-level children plus recovered nodes that
are a real declaration kind, start at column 0, and do not repeat an offset already claimed. Sorted by
start byte, so source order — and therefore module parenting, `-spec` attachment, and doc attachment —
is preserved.

**Uniform walks (the preferred option, taken).** Symbols, types, relationships, and identifiers all read
that one list, so recovered functions get export-driven visibility, arity grouping, clause collapse, and
doc attachment through the normal paths. `recovered_functions_are_walked_for_identifiers_and_relationships`
asserts a call inside a recovered function produces an identifier and a remote call produces a pending
edge. No symbols-without-refs half-coverage.

**One honest caveat.** `walkable()` skips a declaration fully contained in an earlier one. A damaged parse
can leave a wide `fun_decl` that swallowed the forms after it while recovery also rescues one of those
forms precisely; both are real symbols, but walking both would attribute the same bytes twice. In
`telemetry.erl` this means `execute/2` (line 360) has its own symbol while the single call in its body is
scoped to `span/3`, whose damaged `fun_decl` covers lines 315–361. One identifier row, slightly
over-broad scope — chosen over duplicate rows. Top-level forms in a clean file never overlap, so this is
the identity there.

### Borrow-checker note

`ErlangExtractor` owns `recovered_trees: Vec<Tree>` because nodes borrow their tree and the four walks run
across separate `&mut self` calls. `with_declarations` moves the vector out of `self` for the duration of
the call so the declaration nodes borrow a local, then moves it back. No `unsafe`, no self-referential
struct, and `extract_erlang` in `registry.rs` is untouched.

### A filter I built, proved redundant, and removed

I added `is_trustworthy_declaration` to stop a recovered `fun_decl` with a damaged argument list from being
published under an invented arity — `broken(( ->` parses as `broken/1` with signature `broken/1(( ->\n )`.
Instrumenting it showed it *fires* on the corpus (four `?DOC("""…""")` forms in `telemetry.erl`), but three
separate probe fixtures all showed it never changes an outcome: the same results are already guaranteed by
resume-point strictness (a broken form's own head is never a resume point, since resume points are strictly
after an error's start), by offset dedup, and by `walkable`'s containment rule. Rather than ship a branch
whose test passes for reasons other than the branch, I removed it and kept the behaviour test
(`a_recovered_function_with_a_damaged_argument_list_is_rejected`) as a regression guard on the outcome. The
invariant is documented on `merge_declarations` instead.

### Tests

`crates/julie-extractors/src/tests/erlang/parse_errors.rs` — 12 tests (was 3):

| Test | Guards |
|---|---|
| `declarations_after_a_parse_error_are_recovered` | `audit/1` after the broken form (the pre-existing fixture used to lose it) |
| `function_after_a_macro_clause_head_is_recovered_with_its_identity` | probe B — signature `g/0()`, Public, `arity=0` |
| `declarations_after_nested_parse_errors_are_recovered` | nested `?WITH_STACKTRACE` inside a `case` inside a `catch` |
| `form_like_lines_inside_triple_quoted_strings_do_not_become_symbols` | exact list `["p","real"]` — `ghost() -> …`, `-record`, `-define` inside `"""` stay invisible |
| `form_like_lines_inside_comment_blocks_do_not_become_symbols` | exact list — commented-out declarations stay invisible |
| `garbage_inside_an_error_region_does_not_synthesize_symbols` | exact list — `) ] } ,, ->> ;; ||| 12345 <<>> #{} $x` yields nothing |
| `a_recovered_function_with_a_damaged_argument_list_is_rejected` | no fabricated arity |
| `recovered_declarations_carry_spans_from_the_original_source` | `audit/1` at line 9, column 0 — the offset-preservation guarantee |
| `recovered_functions_are_walked_for_identifiers_and_relationships` | uniform walks |

`recovery.rs` unit tests (5), including the cost guard:

- `clean_source_does_no_recovery_parses` — the trigger: a clean tree returns an empty vector, so a large
  clean file takes the zero-extra-parse path.
- `recovery_is_bounded_by_the_parse_budget` — 200 broken forms still produce `<= MAX_RECOVERY_PARSES` trees.
- `blanking_preserves_byte_length_and_line_breaks`, `blanking_preserves_offsets_for_multibyte_content`
  (a `☃` in a comment), `resume_points_skip_lines_inside_strings_and_comments`.

### Corpus baseline

Only `telemetry.erl` moves. Diagnostics are unchanged everywhere, and always will be — recovery reads
extra declarations out of re-parses, it does not repair the primary parse the diagnostics come from. The
baseline comment says so.

| Path | symbols before → after | diagnostics |
|---|---|---|
| `telemetry-1.3.0/src/telemetry.erl` | **16 → 24** | 45 (unchanged) |
| every other corpus file | unchanged | unchanged |

`telemetry.erl` 24 = 1 module + 11 `-type` + 12 functions. Hand-reviewed: the 8 declared exports plus the
4 private helpers between them (`assert_event_names/1`, `assert_event_prefix/1`, `assert_event_name/1`,
`merge_ctx/2`), each with a correct signature read from the real source — e.g.
`merge_ctx/2(#{telemetry_span_context := _} = Metadata, _Ctx)`, `assert_event_name/1([_ | _] = List)`.
Multi-clause helpers collapsed correctly (`assert_event_names` has two clauses, one symbol).
Identifiers 20 → 61, relationships 2 → 13, pending 4 → 14 for that file.

The spot-assertion is now a `TELEMETRY_EXPORTS` table of all eight `(name, arity)` pairs, checked against
public function symbols with arity read from `metadata_json`, **plus an exact-count assertion** so a future
change cannot quietly add a ninth public function.

| Export | Line | Recovered? |
|---|---|---|
| `attach/4` | 82 | primary parse |
| `attach_many/4` | 112 | primary parse |
| `detach/1` | 133 | primary parse |
| `execute/3` | 156 | primary parse |
| `span/3` | 315 | **recovered** |
| `execute/2` | 360 | **recovered** |
| `list_handlers/1` | 371 | **recovered** |
| `report_cb/1` | 416 | **recovered** |

### Goldens

**Byte-identical — not regenerated.** `git status` shows no change under `fixtures/extraction/`, and
`cargo xtask test golden` passes. Expected: golden fixtures parse clean, so `recover` returns before
touching the parser.

### Verification

| Check | Result |
|---|---|
| `cargo xtask test language erlang` | exit 0 — 99 passed |
| `cargo xtask test golden` | exit 0 — 3 passed, no fixture diff |
| `cargo xtask test capability` | exit 0 — 39 + 1 passed |
| `cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus` | 3 passed, **3.47s** corpus scan |
| `cargo test -p julie-extract-cli` (default) | 279 passed; `erlang_corpus.rs` still `running 0 tests` |
| `cargo xtask test default` | exit 0, 23 `test result: ok` |
| `cargo xtask test contract` | exit 0 |
| `cargo clippy -p julie-extractors -p julie-extract-cli --all-targets` | 0 warnings |
| `cargo clippy -p julie-extract-cli --features test-real-world --all-targets` | 0 warnings |
| `cargo fmt --all -- --check` | clean |

Corpus scan wall-time 3.34s → 3.47s (+4%), all of it `telemetry.erl`'s six recovery passes. The 615 KB
`unicode_util_compat.erl` parses clean and does zero extra work.

### Concerns

- **`walkable` containment** is the one behavioural compromise; scope attribution for a recovered form
  nested inside a damaged one is slightly over-broad. Documented above and in the code.
- **`starts_form` assumes column-0 top-level forms.** Universal Erlang convention and what `erlfmt`
  produces, but a file that indents top-level forms would recover less. It only ever affects files that
  already fail to parse — a clean file is untouched.
- **Recovery is Erlang-only.** Other languages with preprocessor-shaped breakage (C/C++ macros) have the
  same class of problem and no equivalent. Worth a look under the language-parity rule, but out of scope
  here and not something this branch regressed.
- **`MAX_RECOVERY_PARSES = 32` is a judgement call**, not a measured optimum. `telemetry.erl` converges in
  6. A pathological file would stop at 32 and silently recover less; there is no diagnostic for hitting
  the cap. Adding one would mean a new diagnostic kind, which felt out of scope for this round.
