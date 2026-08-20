# Test-detection golden closure

Date: 2026-07-09

**Status update 2026-08-19 (v2.34.2):** seven leftover test-role cells
from this ledger are closed. QML, GDScript, Bash, and Scala now have
golden-backed `test_lifecycle` (and QML/GDScript `test_container`). R
`test_lifecycle` is `not_applicable` — testthat has no per-call lifecycle
DSL. Live leftover ranking is
[2026-08-19-test-role-lifecycle-flags.md](2026-08-19-test-role-lifecycle-flags.md).
The tables below keep the 2026-07-09 snapshot; the rows this release
changed are marked `(closed v2.34.2)`.

## Outcome

The fixed `test_detection` vocabulary is now fully classified for all 36
languages: 60 role claims are golden-backed `supported`, 6 roles are
source-backed `not_applicable`, and 42 role variants remain explicit
`open_gaps`. There are no silent cells.

`test_detection` is now a strict code-language expectation. Every executable
language resolves at least one role through golden evidence; remaining role
variants stay visible as owned gaps rather than being treated as implied
support. The strict report closes at `silent_cells: 0` and
`quality_bar_debts: 0`.

The ledger below was generated from the live capability matrix and every
registered expected artifact. A role appears as supported only when the named
golden contains its contract field. `—` means the role is not supported; the
open-gap and not-applicable ledgers explain every such cell.

## Supported-role evidence ledger

| Language | `test_case` golden | `test_container` golden | `test_lifecycle` golden |
| --- | --- | --- | --- |
| rust | `rust:test_roles` | — | — |
| c | `c:test_roles` | — | — |
| cpp | `cpp:test_roles` | `cpp:test_roles` | — |
| go | `go:test_roles` | `go:test_roles` | `go:test_roles` |
| zig | `zig:test_roles` | — | — |
| typescript | `typescript:test_roles` | `typescript:test_roles` | `typescript:test_roles` |
| tsx | `tsx:test_roles` | `tsx:test_roles` | `tsx:test_roles` |
| javascript | `javascript:test_roles` | `javascript:test_roles` | `javascript:test_roles` |
| jsx | `jsx:test_roles` | `jsx:test_roles` | `jsx:test_roles` |
| html | — | — | — |
| css | — | — | — |
| vue | `vue:test_roles` | `vue:test_roles` | `vue:test_roles` |
| python | `python:test_roles` | — | — |
| java | `java:test_roles` | — | — |
| csharp | `csharp:test_roles` | — | — |
| vbnet | `vbnet:basic`, `vbnet:test_roles` | — | — |
| php | `php:test_roles` | `php:test_roles` | `php:test_roles` |
| ruby | `ruby:test_roles` | `ruby:test_roles` | `ruby:test_roles` |
| swift | `swift:test_roles` | `swift:test_roles` | `swift:test_roles` |
| kotlin | `kotlin:test_roles` | `kotlin:test_roles` | `kotlin:test_roles` |
| scala | `scala:test_roles` | `scala:test_roles` | `scala:test_roles` (closed v2.34.2) |
| dart | `dart:test_roles` | `dart:test_roles` | `dart:test_roles` |
| elixir | `elixir:test_roles` | `elixir:test_roles` | `elixir:test_roles` |
| lua | `lua:test_roles` | `lua:test_roles` | `lua:test_roles` |
| qml | `qml:test_roles` | `qml:test_roles` (closed v2.34.2) | `qml:test_roles` (closed v2.34.2) |
| r | `r:test_roles` | `r:test_roles` | not_applicable (closed v2.34.2) |
| bash | `bash:test_roles` | `bash:test_roles` | `bash:test_roles` (closed v2.34.2) |
| powershell | `powershell:test_roles` | `powershell:test_roles` | `powershell:test_roles` |
| gdscript | `gdscript:test_roles` | `gdscript:test_roles` (closed v2.34.2) | `gdscript:test_roles` (closed v2.34.2) |
| razor | `razor:test_roles` | — | — |
| sql | — | — | — |
| regex | — | — | — |
| markdown | — | — | — |
| json | — | — | — |
| toml | — | — | — |
| yaml | — | — | — |

Fixture keys resolve through each language's `fixtures[]` entries in
`fixtures/extraction/capabilities.json`. The bidirectional capability guard
independently fails if a supported role lacks a registered golden or a golden
role is not advertised.

## Remaining owned gaps

All 42 gaps below have role-specific `reason`, `required_closure`, and
`planned_closure_task` fields in the capability matrix. Every closure requires
language-native applicability to be decided first, similar non-test controls,
and registered golden proof before promotion.

| Language | Open roles | Required closure surface |
| --- | --- | --- |
| rust | `test_container`, `test_lifecycle` | Establish stable Rust container and fixture-hook signals without inventing roles from modules or helpers. |
| c | `test_container`, `test_lifecycle` | Model a real C framework container and Unity/CMocka-style hooks without treating Criterion name arguments as suite symbols. |
| cpp | `test_lifecycle` | Prove and classify supported C++ fixture setup/teardown hooks. |
| zig | `test_container`, `test_lifecycle` | Establish stable Zig grouping and lifecycle conventions with ordinary-symbol controls. |
| html | `test_case`, `test_container`, `test_lifecycle` | Select a named markup or embedded-script framework contract and prove it against similar non-test HTML. |
| python | `test_container`, `test_lifecycle` | Promote supported unittest/pytest classes and setup/teardown hooks beyond callable-only `is_test` detection. |
| java | `test_container`, `test_lifecycle` | Add type-level JUnit/TestNG containers and lifecycle-role promotion for supported annotations. |
| csharp | `test_container`, `test_lifecycle` | Add xUnit/NUnit/MSTest type-level containers and lifecycle-role promotion. |
| vbnet | `test_container`, `test_lifecycle` | Add VB.NET xUnit/NUnit/MSTest type-level containers and lifecycle-role promotion. |
| scala | `test_lifecycle` | Closed v2.34.2: ScalaTest `beforeEach` / `afterEach` / `beforeAll` / `afterAll`. |
| qml | `test_container`, `test_lifecycle` | Closed v2.34.2: `TestCase` container; `initTestCase` / `cleanupTestCase` / `init` / `cleanup`. |
| r | `test_lifecycle` | Closed v2.34.2 as `not_applicable`: testthat has no per-call lifecycle DSL. |
| bash | `test_lifecycle` | Closed v2.34.2: `setup` / `teardown`. |
| gdscript | `test_container`, `test_lifecycle` | Closed v2.34.2: `GutTest` container; `before_each` / `after_each` / `before_all` / `after_all`. |
| razor | `test_container`, `test_lifecycle` | Extend the embedded-C# boundary to type-level containers and lifecycle-role promotion. |
| sql | `test_case`, `test_container`, `test_lifecycle` | Select one named SQL testing framework contract and prove routines/schemas/hooks with negative controls. |
| markdown | `test_case`, `test_container`, `test_lifecycle` | Select one named documentation-test contract for fences, sections, or metadata. |
| json | `test_case`, `test_container`, `test_lifecycle` | Select one named JSON test schema and distinguish it from structurally similar data. |
| toml | `test_case`, `test_container`, `test_lifecycle` | Select one named TOML test schema and distinguish it from ordinary tables and keys. |
| yaml | `test_case`, `test_container`, `test_lifecycle` | Select one named YAML test schema and distinguish it from ordinary mappings and sequences. |

## Source-backed not-applicable classifications

| Language | Roles | Source basis |
| --- | --- | --- |
| css | `test_case`, `test_container`, `test_lifecycle` | The complete pinned CSS grammar and language-local visitor model stylesheets, selectors, declarations, values, at-rules, and custom properties. None is an executable case, grouping construct, or setup/teardown hook. |
| regex | `test_case`, `test_container`, `test_lifecycle` | The complete pinned regex grammar and visitor model patterns, groups, assertions, quantifiers, alternation, escapes, backreferences, properties, and conditionals. Test roles belong to a host language or schema, not regex syntax. |
| r | `test_lifecycle` | Added v2.34.2. testthat has no per-call lifecycle DSL (`beforeEach` / `setup()` hooks). File-level `setup.R` / RUnit `.setUp` stay out of this classification. |

The parser versions, complete node inventories, extractor surfaces, and
registered-artifact scan supporting these six negative claims are recorded in
`docs/findings/2026-07-09-test-detection-applicability-audit.md`. No negative
classification was inferred from an empty fixture.

## Plan corrections and detector work

- Language-local tiers do not load the registered golden corpus. Registration
  RED was therefore demonstrated by the golden tier, not by
  `cargo xtask test language <language>`.
- Path-sensitive fixture names were corrected instead of weakening production
  detection: Go uses `source_test.go`; Python, PHP, Ruby, Lua, R, Bash, Swift,
  GDScript, and QML use `test_source.*` where their detectors require a test
  path or filename.
- The PHP negative fixture uses an ordinary object member call without a
  declaration named `test`; on a test path, such a declaration would correctly
  be positive evidence rather than a negative control.
- Razor had one real detector defect. Embedded C# methods persisted normalized
  annotations but passed an empty annotation list to `is_test_symbol`. The
  method path now passes the same normalized keys to detection and persistence;
  a focused regression test and `razor:test_roles` golden prove the fix.
- The Task 2 audit left Vue open-only because Vue preserved embedded
  declarations but did not materialize JS/TS test calls. Promotion correctly
  rejected that state. A language-local Vue adapter now routes both `<script>`
  and `<script setup>` call expressions through the shared JS/TS vocabulary,
  remaps symbol and body spans plus container parents into the host SFC, and
  preserves ordinary declaration and member-call negatives. `vue:test_roles`
  now proves all three roles. Decision 0006 records the reusable embedded-host
  adapter rule.
- No other shared detector vocabulary was broadened to make fixtures pass.

## Strict promotion proof

The permanent capability guard rejects any code language whose
`test_detection` cell contains only open or silent classifications. An
intentional mutation that removed Rust's supported case produced the focused
error `rust kind_coverage.test_detection has only open or silent
classifications`. With `test_detection` added to
`CODE_LANGUAGE_EXPECTATIONS`, the same mutation made the strict report exit 1
with exactly one quality-bar debt: `rust.test_detection open_gap`. Restoring
the golden-backed claim returned both gates to green.

The completed branch gate is:

- `cargo xtask test default`
- `cargo xtask test golden`
- `cargo xtask test capability`
- `cargo xtask test contract`
- `node scripts/language-data-quality-report.mjs --strict`

## Product boundary

This work remains extraction-only: source trees become versioned SQLite or
JSONL artifacts containing role and capability evidence. It adds no watcher,
runner discovery, runner inventory, scheduling, execution, result collection,
freshness policy, impact verdict, dashboard, or continuous-testing runtime.
Miller may consume these deterministic facts for graph candidates; Eros owns
runner inventory and agentic continuous-testing workflows.
