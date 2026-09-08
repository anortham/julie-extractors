# Native return-type identifier parity — 2026-09-08

Source work in `/home/murphy/source/julie-extractors`, `main`, starting `5912bafe185ce2653ce22f86ec962aef615a27f8`. No release or consumer pin change is part of this work.

## Coverage contract

A named type in an explicit function return position emits exactly one `type_usage` identifier at the token byte span. C# and Razor additionally cover nullable, array, generic and qualified return forms without duplicate identifiers. Elixir/Erlang checks use native type specifications; SQL checks PostgreSQL scalar custom `RETURNS result`, not table-result columns or every SQL dialect. These examples prove this contract, not universal syntax coverage.

## Applicable fixtures

| Language | Source fixture | Expected token span (bytes) |
|---|---|---|
| rust | [source](../../fixtures/extraction/rust/return_types/source.rs) | 28–34 |
| c | [source](../../fixtures/extraction/c/return_types/source.c) | 41–47 |
| cpp | [source](../../fixtures/extraction/cpp/return_types/source.cpp) | 17–23 |
| go | [source](../../fixtures/extraction/go/return_types/source.go) | 54–60 |
| zig | [source](../../fixtures/extraction/zig/return_types/source.zig) | 54–60 |
| typescript | [source](../../fixtures/extraction/typescript/return_types/source.ts) | 35–41 |
| tsx | [source](../../fixtures/extraction/tsx/return_types/source.tsx) | 35–41 |
| vue | [source](../../fixtures/extraction/vue/return_types/source.vue) | 54–60 |
| python | [source](../../fixtures/extraction/python/return_types/source.py) | 33–39 |
| java | [source](../../fixtures/extraction/java/return_types/source.java) | 32–38 |
| csharp | [source](../../fixtures/extraction/csharp/return_types/source.cs) | 24–30 |
| vbnet | [source](../../fixtures/extraction/vbnet/return_types/source.vb) | 78–84 |
| php | [source](../../fixtures/extraction/php/return_types/source.php) | 39–45 |
| swift | [source](../../fixtures/extraction/swift/return_types/source.swift) | 31–37 |
| kotlin | [source](../../fixtures/extraction/kotlin/return_types/source.kt) | 25–31 |
| scala | [source](../../fixtures/extraction/scala/return_types/source.scala) | 42–48 |
| dart | [source](../../fixtures/extraction/dart/return_types/source.dart) | 16–22 |
| elixir | [source](../../fixtures/extraction/elixir/return_types/source.ex) | 67–73 |
| fsharp | [source](../../fixtures/extraction/fsharp/return_types/source.fs) | 39–45 |
| erlang | [source](../../fixtures/extraction/erlang/return_types/source.erl) | 63–69 |
| qml | [source](../../fixtures/extraction/qml/return_types/source.qml) | 43–49 |
| gdscript | [source](../../fixtures/extraction/gdscript/return_types/source.gd) | 35–41 |
| razor | [source](../../fixtures/extraction/razor/return_types/source.razor) | 16–22 |
| sql | [source](../../fixtures/extraction/sql/return_types/source.sql) | 62–68 |
| powershell | [source](../../fixtures/extraction/powershell/return_types/source.ps1) | 34–40 |

## Native non-applicability

These are syntax facts, not missing implementations relabeled as non-applicable. Embedded typed/component languages and annotation contracts are separate from a native return-type position.

| Language | Reason |
|---|---|
| javascript | No native return-type annotation; TypeScript is covered separately. |
| jsx | JSX keeps JavaScript function syntax; typed JSX is covered as TSX. |
| html | No native function declaration; bound JavaScript templates have separate htmx fixtures. |
| css | Stylesheet declarations have no function return-type annotation. |
| ruby | Native Ruby methods have no return-type annotations; RBS is not this parser. |
| lua | Native Lua functions have no return-type annotations; comment annotations are separate. |
| qmldir | Module descriptor, not QML function syntax (QML covered separately). |
| r | Native R functions have no return-type annotations. |
| bash | Shell functions have no declared return types (numeric exit status is not a type reference). |
| regex | Pattern language has no function return-type declaration. |
| markdown | Document format; fenced embedded source is not a native function declaration. |
| json | Data format has no function return-type declaration. |
| toml | Data format has no function return-type declaration. |
| yaml | Data format has no function return-type declaration. |
| xml | Markup format has no native function return-type declaration. |

## Verification

Initial real full scan: 40 languages, zero failed files; 18 of the initial 24 explicit-return probes passed after C#/Razor repair. Additional failures in Go, Zig, VB.NET, Elixir, Erlang and SQL became named regression tests and producer fixes. Final results are recorded below after verification.

## Final result

The rebuilt source CLI extracted all 40 fixture languages with zero failed files, errors or warnings. All **25 native-applicable named return tokens** have exactly one `type_usage` row at the expected byte span; the other 15 languages have explicit native-syntax non-applicability above. PowerShell class methods were added after correcting an initial function-only applicability assumption.

Real artifact: `/tmp/miller-return-final40.db`; extraction JSON: `/tmp/miller-return-final40-extract.json`; complete span assertions and `SELECT language, kind, COUNT(*) FROM identifiers GROUP BY 1,2` evidence: [durable evidence JSON](2026-09-08-return-type-parity.json). The fixture sources and golden expectations are durable under `fixtures/extraction/<language>/return_types/`; temporary artifacts are reproducible.

Shared named-return regressions: 23 passed; C#/Razor bare and wrapped return regression: 1 passed covering both languages. Focused suites: C# identifier14, Razor109, Go107, Zig64, VB.NET99, PowerShell69, Elixir56, Erlang137, SQL83. All 25 native-language golden updates were reviewed and then verified without update mode. No release or Miller pin change was made.

A later canonical direct JSX regression found that the byte-level attribute scanner truncated nested template expressions at whitespace. The htmx emitter now takes the complete existing parsed `jsx_expression` value and attribute span. JS/JSX/TSX canonical cases with nested calls, quoted parentheses and slashes pass; ordinary literal values stay literal. Ten agent-usefulness tests and six affected language golden verifications pass. The rebuilt `julie-extract-cli` binary then force-scanned the40language matrix again: all25 token-span assertions still pass, zero extraction errors/warnings. Final reproducible artifacts for that source state live under `/home/murphy/.cache/miller-dogfood/return-final40.{db,json}`.


## Final producer branch-gate repairs

The default gate identified an unbounded recursive template-span collector. It now uses the shared tree traversal depth budget and propagates exhaustion as unknown template evidence; it never promotes a truncated scan to a partial normalized route. The new exhaustion regression and all four template-helper tests pass. The structural-fact contract JSON was regenerated from the authoritative registry and reviewed: only planned ASP.NET Razor/verb uncertainty and htmx template/consumed-attribute metadata changes appear.

The corpus import fixture now expects602 files, up from538: exactly64 newly added files (25 return-type source/golden pairs and seven agent-usefulness source/golden pairs). Its focused import test passes. A separate capacity-refusal test assumed less than1TB free disk; the isolated dogfood cache has more. Its sparse probe now derives apparent length from actual available capacity plus1GiB, preserving refusal/no-mutation checks without allocating that content. The narrow capacity test passes. No production storage logic changed.

Default-tier coverage is composed from unchanged green harnesses, the two repaired fixture cases, all remaining CLI harnesses and CLI doc tests:4853 passed,11 ignored, zero unresolved failures. Original failures remain in the logs; this is not described as one all-green `xtask` invocation. Exact log/count ledger: `/home/murphy/.cache/miller-dogfood/producer-default-composed.json`.


The complete contract tier is also covered:376 passed,3 ignored, zero unresolved failures. This combines golden7, capability39 (37 unchanged passes plus two repaired Razor capability-claim cases),18 remaining command groups totaling186 passes, and144 identical contract checks already passed in the default tier. The final maintenance equivalence test performed a real full-level import of every supported language and public generation promotion. No fixture fact deltas appeared after the depth guard. Exact command/log composition: [branch-gate evidence JSON](2026-09-08-producer-branch-gates.json). `cargo fmt --all --check` and `git diff --check` pass.
