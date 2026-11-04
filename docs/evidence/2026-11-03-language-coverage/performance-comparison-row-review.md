# Performance comparison row review

## Decision scope

The exact row allowlist covers only deltas observed in untimed semantic preflight against the pinned original binary and final candidate binary. The comparison primitive retained all durable SQLite tables, typed columns, row multiplicity, owners, arrays, reference rows, diagnostics, and metadata. It omitted only the harness contract’s documented scan-history tables and volatile file columns.

No latency, peak-RSS, artifact-size, or retry measurements were run. The final controlled measurement remains with the lead after the Windows run and host/build activity finish.

## Provenance pins

| Input | Path | SHA-256 |
|---|---|---|
| Baseline binary | target/coverage-baseline/julie-extract-3.11.0 | 79eb21fa95d39445e78b7237a0300fe071649d5671db0fbc4b3baff0ee2fd78d |
| Baseline build metadata | docs/evidence/2026-11-03-language-coverage/baseline-build.json | d8e35488379f897a6b334343349184398d5f9f52af54386f28522ece5bd2d1c7 |
| Frozen baseline expectations | docs/evidence/2026-11-03-language-coverage/baseline-receipt/baseline-expectations.json | ae5b1e5ec8500d5b4e7c46b25a6961a9ce56128becfe4e4474f314d90dbf404e |
| Candidate binary | target/release/julie-extract | c236e36111785c9ac6bcab6220f07403ad7ad54d02b00590fbe27f6358e40dcc |
| Candidate build metadata | docs/evidence/2026-11-03-language-coverage/candidate-build.json | 4643c7acdb8a1785725d45c63e1985ad57e58ec60fcbb72525e6e0d891ec2448 |
| Candidate build record | docs/evidence/2026-11-03-language-coverage/candidate-build-record.md | 47a8f039633f42606b21dd14d8fef51fc5270617e4f78db7ba01203ade2e5a1e |
| Coverage manifest | fixtures/real-world/coverage-manifest.json | fb6e6a8a286acd8900094023d5d8c2b110695eb459c35b52bdfb642fd1a58ec6 |
| Output change ledger | docs/contracts/extraction-output-changes.md | aad2a8341fa8ca888b785c9de9161b008af372ff8e3ab1bc4ccda00e22063ecb |
| Untimed preflight | docs/evidence/2026-11-03-language-coverage/performance-comparison-preflight.json | 7f49d81364498dec9c521b95333b75a2f2672a25f4aa15759cee25c26d4d1205 |
| Exact row allowlist | docs/evidence/2026-11-03-language-coverage/performance-comparison-row-allowlist.json | 578ae07fa522f8c0785ef08b86b3215421947ad8f56957c899d8815859cefacb |
| Per-entry state evidence | docs/evidence/2026-11-03-language-coverage/performance-comparison-row-evidence.json | d3b813fe1773d3e6bdb97bdd89172dc8d1648f0fd45f3b295559a34843bafc96 |

The candidate metadata was produced by a clean release build at source commit 4aac8d123434167ccbe20b06ac4ea21ed7e493a8 with source tree SHA-256 195783c00bb38c4adf69c4777aafbda2dd68da1d3178d39eca8959a54306d911. The current uncommitted changes after that build are documentation, tests, and evidence; the binary and build sidecar hashes still match the preflight.

The manifest stages 42 languages and 2 controls (44 inputs). The original state SHA-256 is cf98cbd5de935cd445895b501ee0800f23760c395046774da3e5f96d59438ab6. The update state SHA-256 is 586ad6ed5ace7b6bc68ce44b0f5323cc76d5a1dfb70b04cc62e0068ea0bdc700; it appends one newline to zig/isnan.zig. The two states produce identical row-delta multisets for references and full.

## Row totals

Each cell is removed rows / added rows. The literal is present at both extraction levels. Complexity metrics and source regions are full-only.

| Table | References | Full | Source group |
|---|---:|---:|---|
| complexity_metrics | 0 / 0 | 2 / 2 | SQL procedure and file scopes |
| identifiers | 73 / 104 | 73 / 104 | SQL procedure; Erlang variable access |
| language_capabilities | 28 / 28 | 28 / 28 | One snapshot pair for 28 languages |
| language_capability_fixtures | 0 / 8 | 0 / 8 | Eight new registered regression fixtures |
| literals | 0 / 1 | 0 / 1 | SQL PRINT string at line 59 |
| parse_diagnostics | 9 / 0 | 9 / 0 | SQL procedure recovery diagnostics |
| parser_inventory | 4 / 4 | 4 / 4 | Erlang, PowerShell, SQL, VB.NET |
| pending_relationships | 8 / 9 | 8 / 9 | SQL procedure calls and references |
| reference_sites | 8 / 9 | 81 / 113 | SQL references; full also adds 29 Erlang variable reads |
| source_regions | 0 / 0 | 0 / 1 | SQL PRINT string at line 59 |
| structural_facts | 11 / 16 | 11 / 16 | SQL queries, procedure, transactions, table variable |
| symbols | 49 / 50 | 49 / 50 | Body/hash fixes across corpus and SQL routine facts |
| type_facts | 2 / 3 | 2 / 3 | SQL routine, parameter, table variable |

The references comparison has 424 row-delta instances in each source state. Full has 606. Across levels there are 783 distinct exact table/action/canonical-row/count tuples. The allowlist has 3,090 entries: one exact row entry per level and workload. Single-file-update entries are evidenced against both original and updated states. The repeated canonical rows are intentionally scoped to references/full and fresh_scan/no_change_scan/single_file_update. Every entry has a unique ID, exact canonical row, row hash, count, and one output-ledger marker.

## Symbol body and hash rows

Twenty-seven paired symbol owners change body spans and their derived hashes. The listed count is owner pairs; each pair contributes one remove and one add row.

| Corpus path | Pairs | Observed change |
|---|---:|---|
| erlang/telemetry_handler_table.erl | 1 | Module body removed |
| javascript/counter.js | 1 | innerHTML property body removed |
| jsx/App.jsx | 1 | useState import body removed |
| powershell/RegisterMicrosoftUpdate.ps1 | 3 | Non-callable variable bodies removed |
| r/scale-identity.R | 11 | Nine parameter bodies and two non-callable ggproto values corrected |
| swift/CollectionExtensions.swift | 2 | Closure-parameter bodies removed |
| tsx/App.tsx | 1 | useState import body removed |
| typescript/utils.ts | 7 | Non-callable value/import bodies removed |

Seventeen other paired symbol owners are hash-only changes: Bash 1, C# 2, Dart 2, F# 5, JavaScript 3, Python 2, and TypeScript 2. Their exact paths and rows are retained in the allowlist. These entries use syntax-aware-body-hash-v1. Swift body-span entries use declaration-body-ownership-v1. Other source-local fact and body-span entries use language-coverage-corrections-v1.

## Capability snapshots and parser provenance

The capability table has one remove/add pair for each of Bash, C++, C#, CSS, Elixir, Erlang, GDScript, HTML, Java, JavaScript, JSX, Kotlin, Lua, PowerShell, Python, QML, qmldir, R, Razor, regex, Ruby, Rust, SQL, TSX, TypeScript, VB.NET, Vue, and XML. All 28 change kind_coverage_json. Erlang, PowerShell, SQL, and VB.NET also change dependency_status to vendored.

The changed capability domains are:

- Bash: body_spans, identifiers, relationships
- C++: body_spans, symbols
- C#: relationships, symbols
- CSS: body_spans
- Elixir: body_spans, relationships, symbols
- Erlang: body_spans
- GDScript: body_spans, symbols
- HTML: annotations, body_spans, relationships, symbols
- Java: body_spans
- JavaScript: body_spans
- JSX: body_spans, relationships
- Kotlin: body_spans
- Lua: relationships
- PowerShell: body_spans, relationships
- Python: body_spans
- QML and qmldir: body_spans
- R: body_spans, relationships
- Razor: body_spans, identifiers
- regex: body_spans, symbols
- Ruby: body_spans, relationships, symbols
- Rust: body_spans
- SQL: body_spans, symbols
- TSX: body_spans, relationships
- TypeScript: body_spans
- VB.NET: body_spans, relationships
- Vue: body_spans
- XML: complexity_metrics

The eight fixture registrations are Erlang macro_catch_head; HTML annotations and external_script_body; PowerShell variable_path_arguments; SQL tsql_coverage_regressions; Swift source_regions; VB.NET parameter_attributes; and XML complexity. The HTML external-script fixture row uses html-external-script-body-v1. Other capability and fixture rows use language-coverage-corrections-v1.

The parser inventory removes the old entries and adds these vendored versions:

| Language | Removed | Added |
|---|---|---|
| Erlang | 0.20.0, registry source | 0.21.0-julie.1, vendored |
| PowerShell | 0.26.4, git-pinned source | 0.26.6-julie.1, vendored |
| SQL | 0.4.2, git-pinned source | 0.4.2-julie.1, vendored |
| VB.NET | 0.1.0, git-pinned source | 0.1.0-julie.1, vendored |

## SQL source-local row causes

Every SQL row delta in this corpus points to fixtures/real-world/coverage/sql/InsertCustomerOrders.sql.

The candidate recognizes the declared Website.InsertCustomerOrders procedure through its full body, ending at line 65 / byte 2362. Its parameters remain separate from WITH EXECUTE AS OWNER. The corrected symbol metadata records schema Website. Three parameters are re-keyed under the corrected procedure symbol. The @SalespersonPersonID signature ends at its int type. The DECLARE @OrdersToGenerate AS TABLE form produces a TABLE variable symbol and type fact.

- Symbols: five old rows pair with five corrected rows; the new @OrdersToGenerate variable adds one. Changes include procedure extent/signature/schema, the salesperson parameter signature/extent, and three parameter parent-symbol links.
- Type facts: the procedure type is re-keyed; @SalespersonPersonID changes from the recovered text int WITH EXECUTE OWNER to int; @OrdersToGenerate adds TABLE.
- Pending relationships: eight old rows pair under the new procedure owner; XACT_STATE adds one pending call from line 58.
- Structural facts: nine rows pair after owner changes. The procedure definition and first SELECT spans are corrected. Three transaction actions are added at lines 27, 54, and 58; two table-variable column facts are added at lines 15 and 16.
- Complexity metrics: two rows pair at file and procedure scope. Decision count changes from 4 to 5. Procedure coverage moves from 49 lines / 1,972 bytes to 58 lines / 2,155 bytes; file coverage stays 64 lines / 2,359 bytes.
- Diagnostics: the nine old SQL error rows disappear; the current manifest pins zero SQL diagnostics.
- Literal and source region: the PRINT text Unable to create the customer orders. at line 59 adds one literal and one string_literal source region. The literal is present at both references and full levels; the source region is full-only.
- Identifier/reference rows: 72 SQL identifier rows and 80 SQL reference-site rows pair when ignoring IDs and parent owner keys. The only removed unmatched SQL token is NEXT at line 22 in NEXT VALUE FOR Sequences.OrderID. The grammar defines this as sequence-value syntax, so NEXT no longer emits a false member_access identifier/reference. Two type-usage identifiers are added for OrderList and OrderLineList; XACT_STATE adds a call identifier/reference. The output ledger records this exact correction under language-coverage-corrections-v1.

The candidate corpus runner reported 44 inputs passed with no failed or unsupported files. Its one malformed-Rust diagnostic is pinned by the manifest. SQL diagnostics are exactly zero.

## Per-language selection

The required final run is the all-language comparison. For a selected-language run, use a filtered sidecar containing only that language’s exact rows. Most tables expose a language column, so retain entries whose row language matches the selected manifest slug. pending_relationships has no language column; filter those exact rows by the first path component in the fixed manifest. Passing the aggregate sidecar unchanged to an unrelated single-language run would fail unused-entry validation.

## Exact measurement command

Run only after the lead confirms the host is idle and the Windows run has finished. No budget-approval record is supplied, so default budgets apply.

    cargo xtask performance compare \
      --baseline target/coverage-baseline/julie-extract-3.11.0 \
      --candidate target/release/julie-extract \
      --baseline-expectations docs/evidence/2026-11-03-language-coverage/baseline-receipt/baseline-expectations.json \
      --baseline-expectations-sha256 ae5b1e5ec8500d5b4e7c46b25a6961a9ce56128becfe4e4474f314d90dbf404e \
      --baseline-build-metadata docs/evidence/2026-11-03-language-coverage/baseline-build.json \
      --baseline-build-metadata-sha256 d8e35488379f897a6b334343349184398d5f9f52af54386f28522ece5bd2d1c7 \
      --candidate-build-metadata docs/evidence/2026-11-03-language-coverage/candidate-build.json \
      --candidate-build-metadata-sha256 4643c7acdb8a1785725d45c63e1985ad57e58ec60fcbb72525e6e0d891ec2448 \
      --manifest fixtures/real-world/coverage-manifest.json \
      --allowlist docs/evidence/2026-11-03-language-coverage/performance-comparison-row-allowlist.json \
      --profile release \
      --out-dir target/performance-compare-task5-final-4aac8d12

The final binary and sidecar hashes matched the untimed preflight. The manual harness validation supplied the full pinned command above but used existing target as the output directory. It exited with the expected “output directory must be new” error after binary/build/manifest/receipt/allowlist/ledger preflight and before creating workload stages or running scans. Log: target/task5-row-allowlist-harness-preflight.log.

## Verification and remaining work

- Untimed semantic preflight: references/full, original/updated source states, all 44 inputs; baseline semantic dump hashes matched the frozen receipt.
- Allowlist static scope check: 3,090 entries; exact rows, hashes, counts, markers, source states, and no overlapping entries; all 4 level/state groups reproduced exactly for each workload scope.
- Actual harness preflight: accepted the full allowlist and all hash-bound provenance, then stopped on the intentionally existing output directory. No timed measurements ran.
- Pending: lead review of the generated allowlist/evidence and controlled all-language timing, RSS, size, retry, and budget results after Windows and host/build activity are complete.
