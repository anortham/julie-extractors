# Rust support

Julie registers `rust` for `.rs` files. The extractor lives in
`crates/julie-extractors/src/rust/`. Test roles are written from attribute
macros only; the rules are shared with no other language.

## Continuous testing

Run the language target when changing Rust extraction:

```bash
cargo xtask test language rust
```

The command runs the Rust unit-test modules and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=rust`. The normal golden target remains unfiltered:

```bash
cargo xtask test golden
```

The Rust test-role rules also have unit tests outside the `rust` modules,
because they live in the shared detector:

```bash
cargo test -p julie-extractors --lib rust
```

## Test roles

Detection is annotation-only. A Rust function earns a role from an attribute
macro, never from its name and never from its path. `fn test_parser` with no
attribute is ordinary code, and `#[test]` in `src/lib.rs` is a real case.

Attribute keys arrive lower-cased with the `#[` and `]` stripped and the
argument list removed, so `#[test_case(1, 2)]` produces the key `test_case` and
`#[tokio::test]` produces `tokio::test`. The path is kept: Rust is not one of
the languages that reduce a key to its rightmost name.

### The `::test` suffix rule

Any attribute macro whose **last `::` segment is exactly `test`** is a test
attribute. That single rule covers every async and framework replacement for
`#[test]` without naming each crate:

| Attribute | Key | Matches because |
| --- | --- | --- |
| `#[test]` | `test` | the whole key is `test` |
| `#[tokio::test]` | `tokio::test` | last segment is `test` |
| `#[actix_web::test]`, `#[actix_rt::test]` | `actix_web::test`, `actix_rt::test` | last segment is `test` |
| `#[sqlx::test]` | `sqlx::test` | last segment is `test` |
| `#[async_std::test]` | `async_std::test` | last segment is `test` |
| `#[googletest::test]` | `googletest::test` | last segment is `test` |
| `#[test_log::test]` | `test_log::test` | last segment is `test` |

The segment must match whole. `latest`, `contest`, `test_util`, `attest`, and
`tokio::main` all stay production attributes. Unit tests in
`crates/julie-extractors/src/tests/test_detection.rs` hold that control list.

### Named attribute macros

Four more macros are recognised by name, matched on the same last segment so
`rstest::rstest` reads the same as `rstest`:

| Role | Attribute keys |
| --- | --- |
| `test_case` | `test`, `wasm_bindgen_test`, `quickcheck`, `proptest`, `gtest`, `traced_test`, `rstest` |
| `parameterized_test` | `test_case`, and `rstest` carrying `#[case]` attributes |
| `fixture_setup` | `fixture` |

`parameterized_test` means the runner reports one result per data row instead
of one result per function. `#[test_case(..)]` is parameterized by definition.
An `#[rstest]` is parameterized only when it carries at least one `#[case]`
attribute; a bare `#[rstest]` stays `test_case`.

rstest names a case by suffixing the attribute, so `#[case::six_times_seven(6,
7)]` is one case. The case attribute is therefore matched on its **leading**
segment, the opposite of the suffix rule above. Matching it on the last segment
instead was measured against the rstest project itself and missed 76 of its 141
parameterized functions, because most of that project's cases are named.

### `#[fixture]` and the lifecycle reversal

The Rust row in `docs/decisions/2026-08-20-test-role-contract-closure.md`
previously read `test_lifecycle: not applicable`. That conclusion held only
while the row named `cfg(test)` as the whole contract. rstest's `#[fixture]`
builds a value a test case asks for by name, it runs only inside a test
session, and Miller's Rust continuous testing provider must know that editing a
fixture invalidates every case that requests it. The row is now
`test_lifecycle: supported`.

A fixture that returns a guard also tears down after the case. The extractor
cannot tell a guard-returning fixture from a plain one without reading the
body, and the setup half always runs, so the contract publishes the single
honest direction: `fixture_setup`.

Rust has no teardown attribute, so `fixture_teardown` is never written for
Rust. That is a narrower claim than the `test_lifecycle` ledger cell can
express, because one cell covers both directions.

### `cfg` module containers

A module marked with a `cfg` predicate that selects test builds is a
`test_container`:

- `#[cfg(test)]` — the bare form
- `#[cfg(all(test, feature = "slow"))]` and `#[cfg(any(test, miri))]` — the
  compound forms, at any nesting depth
- `#[cfg(not(test))]` and `#[cfg(all(unix, not(test)))]` — **not** containers,
  because such a module is compiled out of test builds

A module also publishes its own `cfg` attribute now. Module annotations were
previously dropped, so a consumer could see `test_container: true` with no
attribute row explaining it.

Rust test functions are not scoped to a container. A function in `tests/` or a
`#[test]` beside production code is a real case with no enclosing test module,
so the container pass never strips a role a function's own attribute earned.

### Recorded gaps

One named Rust test surface is not classified. It is recorded as
`open_gaps` on the rust row in `fixtures/extraction/capabilities.json`:

- `rust.benchmark_harness_roles` — nightly `#[bench]`, criterion, and divan.
  `#[bench]` is an attribute macro, so adding it to the list above would report
  it as `test_case`, which is wrong: a benchmark measures time and reports no
  pass or fail. Criterion and divan declare their case lists inside
  `criterion_group!`/`criterion_main!` and `#[divan::bench]`, so the cases live
  in a macro invocation rather than in a callable symbol the role writer can
  reach.
The remaining gap sits under `kind_coverage.structural_facts.open_gaps` rather than
`test_detection`, because the `test_detection` vocabulary is frozen to
`test_case`, `test_container`, and `test_lifecycle` and each of those is
already classified exactly once for rust.

Rustdoc executable fences are emitted as `rust.doc_test.v1` structural facts.
Untagged and `rust` fences carry `mode: "run"`; `ignore`, `no_run`, and
`compile_fail` are preserved as explicit modes. `text` and non-Rust fences are
silent, and facts retain their source fence span without creating a symbol or
test role.

One smaller under-report is not a separate gap row. rstest also builds a case
matrix from `#[values(..)]`, but that attribute sits on a **parameter**, not on
the function, so it never reaches the attribute keys. Such a function reports
`test_case` rather than `parameterized_test`. tree-sitter-rust does not accept
attributes on parameters at all, so those files also raise parse diagnostics
(see the rstest corpus breakdown below).

## Grammar freshness

The grammar is pinned in `Cargo.lock` to `tree-sitter-rust` version `0.24.2`
from crates.io, checksum
`439e577dbe07423ec2582ac62c7531120dbfccfa6e5f92406f93dd271a120e45`.

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The report could not produce a drift verdict during this work: it stops on the
first failure, and GitHub answered `HTTP 403` for `anortham/tree-sitter-c-sharp`
— the unauthenticated rate-limit response. The pin above comes from
`Cargo.lock`, not from the report. Re-run the report with a GitHub token to get
the verdict.

## Real-world evidence

Two corpora were scanned, because no single project exercises both halves of
the contract. `sqlx` proves the `::test` suffix rule and the compound-`cfg`
container on production code. `rstest` proves `#[fixture]`, `#[case]`, and the
named-case form.

Both were cloned shallowly into temporary directories. No project build
scripts, hooks, or third-party binaries were executed. Both checkouts are
dual-licensed MIT / Apache-2.0.

```bash
CORPUS="$(mktemp -d)"
git clone --depth 1 https://github.com/launchbadge/sqlx "$CORPUS"
git -C "$CORPUS" checkout --detach 1d15be8a5fd1d1bcdd37fb9e8aa3c9c2971b24fb

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

The same commands were run for
`https://github.com/la10736/rstest` at commit
`d9ae990e323b6910d39364d94876a86b003b4729`.

Both scan reports were `status=ok` with `files_failed=0`, empty `errors`, and
empty `warnings`. sqlx reported `files_scanned=723` and
`files_unsupported=60`; rstest reported `files_scanned=231` and
`files_unsupported=24`. Per-language counts below come from the SQLite
artifacts.

| Artifact evidence, `rust` rows | sqlx | rstest |
| --- | ---: | ---: |
| Indexed files | 456 | 149 |
| Symbols | 9,668 | 2,500 |
| Identifiers | 83,034 | 17,683 |
| Resolved relationships | 863 | 405 |
| Pending relationships | 16,646 | 4,602 |
| Complexity metrics | 3,481 | 1,672 |
| Structural facts | 150 | 2 |
| Symbol annotations | 2,375 | 1,704 |
| Parse diagnostics | 9 | 47 |

### Test-role evidence

| Role | sqlx | rstest |
| --- | ---: | ---: |
| `test_case` | 642 | 616 |
| `parameterized_test` | 0 | 141 |
| `fixture_setup` | 0 | 170 |
| `test_container` | 23 | 28 |

The old rule matched only `test`, `tokio::test`, and `rstest`. Measured against
the same two artifacts, it published 265 cases for sqlx and 752 for rstest.

**Recall.** sqlx gains 377 cases, every one of them from the suffix rule:
`sqlx_macros::test` (329, four of which also carry `#[ignore]`), `sqlx::test`
(47), and `async_std::test` (1). Those 377 functions are the bulk of the
project's test suite and were previously invisible to every consumer. rstest
gains 5 cases — `async_std::test` (4) and `rstest::rstest` (1) — plus 170
`fixture_setup` rows that published no role at all before, and 76 more
functions correctly upgraded to `parameterized_test` by the leading-segment
case rule.

The compound-`cfg` fix adds one container in sqlx:
`sqlx-core/src/config/mod.rs:46`, marked `#[cfg(all(test, feature =
"sqlx-toml"))]`. The other 22 sqlx containers and all 28 rstest containers use
the bare `#[cfg(test)]`.

**Precision.** No classified callable in either artifact lacks a recognised
test attribute — the count of unjustified classifications is 0 in both. The
controls that stay unclassified are real: sqlx carries 17 `#[tokio::main]`
attributes, and rstest carries 4 `#[test_attr]` attributes. Neither earns a
role, even though both keys read like test vocabulary.

### Remaining gaps in the corpora

The two recorded gaps are measurable on these same artifacts:

- sqlx carries 12 `#[bench]` functions, all in
  `sqlx-postgres/src/message/`. All 12 stay unclassified.
- sqlx doc comments hold 117 fences that `cargo test` would run as doc-tests,
  plus 46 fences marked with a non-running language or attribute. rstest holds
  9 runnable fences. The extractor emits nothing for any of them.

### Diagnostic breakdown

sqlx produced 9 `error` diagnostics across 3 of 456 files:
`tests/postgres/types.rs` (6), `sqlx-mysql/src/types/chrono.rs` (2), and
`sqlx-test/src/lib.rs` (1). 453 files parsed clean.

rstest produced 47 `error` diagnostics across 15 of 149 files. Every one of the
15 lives under `rstest/tests/resources/`, which holds macro-input fixtures
written for the rstest expander rather than for rustc's own grammar. They put
attributes on function parameters — `#[case]`, `#[future]`, `#[by_ref]`,
`#[files("..")]`, `#[exclude("..")]` — and tree-sitter-rust does not accept an
attribute in parameter position. That is the same grammar limit that keeps
`#[values(..)]` out of the attribute keys. No file outside
`tests/resources/` produced a diagnostic.

The temporary checkouts and SQLite artifacts were removed after recording this
evidence.
