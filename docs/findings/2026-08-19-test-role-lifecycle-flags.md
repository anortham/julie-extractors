# Test-role lifecycle flags — leftover update

Date: 2026-08-19

Shipped in v2.34.2. Live scorecard after this slice:

```text
languages: 38
silent_cells: 0
quality_bar_debts: 0
open_gap_backlog: 54
  test_detection: 28
  structural_facts: 25
  relationships: 1
```

## Closed in this slice

| Language | Roles | Evidence |
| --- | --- | --- |
| qml | `test_container`, `test_lifecycle` | `TestCase`; `initTestCase` / `cleanupTestCase` / `init` / `cleanup` |
| gdscript | `test_container`, `test_lifecycle` | `GutTest`; `before_each` / `after_each` / `before_all` / `after_all` |
| bash | `test_lifecycle` | `setup` / `teardown` |
| scala | `test_lifecycle` | ScalaTest `beforeEach` / `afterEach` / `beforeAll` / `afterAll` |
| r | `test_lifecycle` → `not_applicable` | testthat has no per-call lifecycle DSL |

## Still open

The cheap metadata flags are done. Remaining `test_detection` leftovers are
mostly data/markup languages that may be `not_applicable`, plus C-family
framework container/lifecycle.

Same-file structural-facts leftovers (Markdown footnotes/task lists, TOML
string style, YAML scalars) are the next cheap extract slice. Do not start a
Miller cross-file join.

The 2026-07-09 golden-closure finding stays as the original snapshot; its
changed rows are marked `(closed v2.34.2)`.
