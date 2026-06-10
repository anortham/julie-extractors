# Language Coverage Review - 2026-06-09 Continuation

Follow-up to `2026-06-09-data-quality-review.md` after branch
`feature/extraction-data-quality` completed the first implementation pass.
This replaces the paused second-pass audit noted in Goldfish checkpoint
`checkpoint_9eb03aac`.

## Current branch state

The branch completed the original extraction-data-quality plan:

- Golden fixtures now pin `structural_facts`, `complexity_metrics`,
  `literals`, `source_regions`, and `type_argument_usages`.
- `kind_coverage` now includes 10 domains:
  `symbols`, `relationships`, `identifiers`, `body_spans`,
  `structural_facts`, `complexity_metrics`, `annotations`, `doc_comments`,
  `literals`, and `source_regions`.
- Complexity metrics expanded from 7 languages to 24.
- Annotation evidence expanded to 17 languages.
- Doc-comment evidence expanded to 25 languages.
- Source-region evidence expanded to 35 languages.
- Literal evidence expanded to 26 languages.
- No current `kind_coverage` positive claim lacks fixture evidence.

This means the remaining work is not a stale-declaration cleanup. The gap is
target quality: many languages still have shallow extractor behavior compared
with the richest languages. The objective is not to document that unevenness;
it is to remove it wherever the language grammar can support better data.

## Fixture-proven domain coverage

Current golden fixture rows by domain:

| Domain | Languages with rows | Notes |
| --- | ---: | --- |
| symbols | 36/36 | Full baseline exists. |
| relationships | 36/36 | Full baseline exists. |
| pending_relationships | 30/36 | Remaining exceptions are mostly format or same-document domains. |
| identifiers | 33/36 | Markdown, JSON, and TOML are documented exceptions. |
| types | 28/36 | Dynamic or data formats have exception rows. |
| body_spans | 35/36 | YAML is the only current miss. |
| source_regions | 35/36 fixture-proven | Regex cell closed via capability `not_applicable`, not golden rows. |
| doc_comments | 25/36 | Stronger, but normalization is still inconsistent. |
| structural_facts | 19/36 | Tier-1, web/framework, and data/document families now covered. |
| complexity_metrics | 28/36 | Third-batch Phase 2 Task 6 added tsx, jsx, vue, razor embedded/web support. |
| annotations | 17/36 | Attribute/decorator support remains patchy outside the first pass. |
| literals | 36/36 | Full baseline exists. |
| type_argument_usages | 1/36 | Type-argument usage evidence is TypeScript-only in goldens. |

## Phase 0 scorecard

After adding the repeatable scorecard and fail-closed capability-matrix policy:

- `silent_cells`: 0
- `quality_bar_debts`: 98
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 23/36
- `structural_facts`: 12/36
- `complexity_metrics`: 24/36
- `annotations`: 9/36
- `literals`: 9/36
- `type_argument_usages`: 1/36

The zero silent-cell count is not a quality win by itself. It means every
remaining gap is now visible as debt that future language-quality work must
close or justify with language semantics.

## Phase 1 C/C++ extractor-depth slice

The first extractor-depth slice closed C and C++ gaps for:

- `annotations`: C `function`; C++ `function`, `method`.
- `doc_comments`: C `function`, `struct`; C++ `class`, `function`, `method`.
- `literals`: C and C++ `other` string-literal carriers.
- `source_regions`: C and C++ now advertise generated `doc_comment` regions.

The slice also fixed quality bugs uncovered by richer fixtures:

- C and C++ attributes no longer bleed onto following functions.
- C/C++ container doc comments no longer bleed onto child fields or methods.
- HTML fixture-header comments no longer become DOCTYPE documentation; HTML
  doc-comment evidence is now a real documented element.
- Bash no longer claims variable doc comments from a shebang.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 92
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 11/36
- `type_argument_usages`: 1/36

## Phase 2 Go literal slice

The second extractor-depth slice closed Go gaps for:

- `literals`: Go `other` string-literal carriers with dotted (`http.Get`) and
  local helper (`observeRun`) callee context in `fixtures/extraction/go/basic`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 91
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 12/36
- `type_argument_usages`: 1/36

## Phase 3 Rust literal slice

The third extractor-depth slice closed Rust gaps for:

- `literals`: Rust `other` string-literal carriers with local helper (`observe_run`,
  `fetch_url`) callee context in `fixtures/extraction/rust/basic`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 90
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 13/36
- `type_argument_usages`: 1/36

## Phase 4 Python literal slice

The fourth extractor-depth slice closed Python gaps for:

- `literals`: Python `other` string-literal carriers with local helper (`observe_run`,
  `fetch_url`) callee context in `fixtures/extraction/python/basic`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 89
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 14/36
- `type_argument_usages`: 1/36

## Phase 5 Java/PHP/Swift literal batch

The fifth extractor-depth slice closed literal gaps for three languages:

- `literals`: Java `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/java/basic`.
- `literals`: PHP `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/php/basic`.
- `literals`: Swift `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/swift/basic`.

**Source-region collateral (not a separate batch):** The literal carrier fixtures
also emit `source_regions` rows of kind `string_literal` because the extractor
already indexes string-literal spans independently of `kind_coverage.literals`.
Removing `string_literal` from `kind_coverage.source_regions.supported` for these
three languages was attempted and blocked: golden exact-match requires the rows in
`expected.json`, and `capability_matrix_source_region_claims_have_fixture_evidence`
requires every observed golden region kind to appear in `supported`. The
`string_literal` entries are therefore fixture-synchronized matrix contract data,
not an intentional source-regions capability expansion. A dedicated
source-regions slice would need either different evidence shapes or a contract
change before those claims can be dropped.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 86
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 17/36
- `type_argument_usages`: 1/36

## Phase 6 VB.NET/Kotlin/Scala/Dart/Elixir literal batch

The sixth extractor-depth slice closed literal gaps for five languages:

- `literals`: VB.NET `other` string-literal carriers with local helper (`ObserveRun`,
  `FetchUrl`) callee context in `fixtures/extraction/vbnet/basic`.
- `literals`: Kotlin `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/kotlin/basic`.
- `literals`: Scala `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/scala/basic`.
- `literals`: Dart `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/dart/basic`.
- `literals`: Elixir `other` string-literal carriers with local helper (`observe_run`,
  `fetch_url`) callee context in `fixtures/extraction/elixir/basic`.

**Source-region collateral (not a separate batch):** The literal carrier fixtures
also emit `source_regions` rows of kind `string_literal` because the extractor
already indexes string-literal spans independently of `kind_coverage.literals`.
For VB.NET, Kotlin, Scala, and Elixir, `kind_coverage.source_regions.supported`
gained `string_literal` to satisfy
`capability_matrix_source_region_claims_have_fixture_evidence` against the new
golden rows. Dart already advertised `string_literal` in source regions; only
the literal gap closed. These entries are fixture-synchronized matrix contract
data, not an intentional source-regions capability expansion.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 81
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 22/36
- `type_argument_usages`: 1/36

## Phase 7 Zig/QML/GDScript/Razor literal batch

The seventh extractor-depth slice closed literal gaps for four languages:

- `literals`: Zig `other` string-literal carriers with local helper (`observe_run`,
  `fetch_url`) callee context in `fixtures/extraction/zig/basic`.
- `literals`: QML `other` string-literal carriers with local helper (`observeRun`,
  `fetchUrl`) callee context in `fixtures/extraction/qml/basic`.
- `literals`: GDScript `other` string-literal carriers with local helper (`observe_run`,
  `fetch_url`) callee context in `fixtures/extraction/gdscript/basic`.
- `literals`: Razor `other` string-literal carriers with local helper (`ObserveRun`,
  `FetchUrl`) callee context in `fixtures/extraction/razor/basic`.

**Source-region collateral (not a separate batch):** All four languages already
advertised `string_literal` in `kind_coverage.source_regions.supported` before
this slice; only the literal gaps closed. No source-regions matrix changes were
required.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 77
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 26/36
- `type_argument_usages`: 1/36

## Phase 8 remaining literal coverage batch

The eighth extractor-depth slice closed literal gaps for ten languages:

- `literals`: TSX/JSX `other` carriers in component/render contexts (`fetch`,
  `observeRun`, `data-action`) in `fixtures/extraction/tsx/basic` and
  `fixtures/extraction/jsx/basic`.
- `literals`: HTML attribute carriers (`href`, `data-action`) in
  `fixtures/extraction/html/basic`.
- `literals`: CSS `url()` path carrier in `fixtures/extraction/css/basic`.
- `literals`: SQL quoted DDL default carrier in `fixtures/extraction/sql/basic`.
- `literals`: Regex pattern fragment carrier (`foo` in `(foo)`) in
  `fixtures/extraction/regex/basic`.
- `literals`: Markdown inline-link destination carrier in
  `fixtures/extraction/markdown/basic`.
- `literals`: JSON/TOML/YAML config scalar carriers (`name`, `api_url`) in each
  language `basic` fixture.

Extractor work in this slice:

- Added `base/config_literals.rs` for data-language scalar capture.
- HTML attribute values, CSS `url()` arguments, SQL `literal` nodes, markdown
  line-based link destinations, and regex capturing-group fragments now emit
  literal rows with useful carriers.

**Source-region collateral (documented):** SQL and YAML golden fixtures emit
`source_regions` rows of kind `string_literal` alongside new literal rows.
`kind_coverage.source_regions.supported` gained `string_literal` for SQL and
YAML to satisfy `capability_matrix_source_region_claims_have_fixture_evidence`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 67
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 11/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 2 Task 6 first-batch complexity metrics

The ninth extractor-depth slice closed `complexity_metrics` gaps for six
languages:

- `complexity_metrics`: Zig, PHP, Ruby, Scala, Elixir, and Lua now emit
  `file` and `symbol` scoped rows in each language `basic` golden fixture.
- Shared engine extensions in `base/complexity_metrics.rs`:
  - Elixir call-target matching for control-flow macros (`if`, `unless`,
    `case`, `cond`, `with`, `for`) plus `stab_clause`, `rescue_block`, and
    `catch_block` nodes.
  - Ruby same-kind parent/child dedup for nested `if`/`for` wrappers.
  - Scala-only symbol-span fallback when `body_span` covers less than half of
    the declaration span (other languages keep using `body_span` when present).
- Per-language complexity unit tests with hand-tallied expectations in
  `crates/julie-extractors/src/tests/{zig,php,ruby,scala,elixir,lua}/complexity.rs`.
- Cross-language guard cases added in `tests/complexity_metrics.rs`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 61
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 18/36
- `annotations`: 11/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 2 Task 6 second-batch complexity metrics

The tenth extractor-depth slice closed `complexity_metrics` gaps for six
languages:

- `complexity_metrics`: VB.NET, R, Bash, PowerShell, GDScript, and QML now
  emit `file` and `symbol` scoped rows in each language `basic` golden fixture.
- Shared engine extensions in `base/complexity_metrics.rs`:
  - VB.NET joined Scala in the declaration-span fallback when `body_span`
    covers less than half of the declaration span (VB.NET currently mis-tags
    return-type fragments as body spans).
  - PowerShell nested `parameter_list` groups under
    `function_parameter_declaration` via `parameter_group_node_kinds`.
- Bash has no AST formal-parameter container; symbol metrics report
  `parameter_count: null` while still counting control flow from function
  bodies.
- R `else if` chains parse as nested `if_statement` nodes in the
  `alternative` field rather than a separate arm kind; R `switch(...)` parses
  as a generic `call` and is counted by callee name.
- GDScript `match_statement` plus `pattern_section` arms follow the
  switch-container-plus-arm convention.
- Per-language complexity unit tests with hand-tallied expectations in
  `crates/julie-extractors/src/tests/{vbnet,r,bash,powershell,gdscript,qml}/complexity.rs`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 55
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 24/36
- `annotations`: 11/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 2 Task 6 third-batch embedded/web complexity metrics

The eleventh extractor-depth slice closed `complexity_metrics` gaps for four
embedded/web languages:

- `complexity_metrics`: TSX, JSX, Vue, and Razor now emit `file` and `symbol`
  scoped rows in each language `basic` golden fixture.
- TSX and JSX reuse `ECMASCRIPT_CONFIG` on their native tree-sitter grammars;
  JSX/TSX markup nodes do not match decision or loop kinds, so markup nesting is
  not counted as code complexity.
- Razor reuses `CSHARP_CONFIG` on the tree-sitter-razor AST; C# control-flow
  nodes inside `@code` blocks are counted while HTML markup nesting is not.
- Vue parses `<script>` / `<script setup>` sections with the embedded TS/JS
  parser and aggregates metrics only across script byte spans; template and
  style sections are excluded from file-scope complexity.
- Per-language complexity unit tests with hand-tallied expectations in
  `crates/julie-extractors/src/tests/{typescript,javascript,vue,razor}/complexity.rs`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 51
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 28/36
- `annotations`: 11/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 1 Task 5 annotation fixture-evidence batch

The twelfth extractor-depth slice closed `annotations` gaps for six languages
whose extractors already emitted normalized markers but lacked golden fixture
evidence. A follow-up pass deepened that evidence so supported kinds match
what existing unit tests already prove:

- `annotations`: PHP `class`, `method`, `property` via `#[Entity]`, `#[Route]`,
  and `#[Required]`; attribute stubs use `#[\Attribute(...)]`.
- `annotations`: VB.NET `method`, `property` via `<TestMethod>` and `<Obsolete>`.
- `annotations`: PowerShell `function` via `[CmdletBinding()]`.
- `annotations`: Scala `class`, `function`, `method`, `property`, `type` via
  `@deprecated`, `@singleton` object, `@tracked` val, `@opaque` type alias,
  and `@ops` extension function.
- `annotations`: Kotlin `class`, `method`, `property`, `type` via `@Deprecated`,
  `@Singleton` object, `@Volatile` property, constructor `@Suppress`, and
  `@Suppress` type alias.
- `annotations`: Swift `struct`, `function`, `module`, `property`, `type`,
  `enum_member` via `@MainActor`, `@available`, `@Published`, typealias, and
  enum-case attributes.

No extractor code changes were required; existing `normalize_annotations` wiring
and per-language unit tests already covered the syntax. This slice added
annotation-bearing symbols to each language `basic` fixture, regenerated
goldens, and moved `kind_coverage.annotations` from `open_gaps` to
fixture-proven `supported` rows.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 45
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 25/36
- `structural_facts`: 12/36
- `complexity_metrics`: 28/36
- `annotations`: 17/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 1 Task 5 remaining annotation audit

The thirteenth extractor-depth slice audited the 19 languages that still
carried generic `attribute_or_decorator` open gaps:

- `annotations`: TSX and JSX now have fixture-proven `class` decorator support
  via the existing TypeScript/JavaScript decorator paths (`@Component()` and
  `@registered` in each language `basic` fixture).
- `annotations`: Eight format/data languages (`html`, `css`, `sql`, `regex`,
  `markdown`, `json`, `toml`, `yaml`) are now `not_applicable` because markup
  attributes, selectors, keys, and pattern syntax belong to other domains, not
  symbol-attached annotation markers.
- `annotations`: Five scripting languages (`ruby`, `lua`, `qml`, `r`, `bash`)
  are now `not_applicable` because the grammars expose no first-class
  attribute/decorator syntax on declarations (R roxygen tags remain in
  `doc_comments`).
- `annotations`: Two code languages retain concrete open gaps: Vue (`class`,
  `function` for script decorators and component metadata), and Razor (`method`,
  `property`, `class` for embedded C# attributes in `@code` blocks).

## Phase 8 Go/Zig annotation slice

The eighth extractor-depth slice closed Go and Zig annotation gaps for:

- `annotations`: Go `struct` (field-tag summary), `field` (per-key struct
  tags), and `function` (`//go:` compiler directives) in
  `fixtures/extraction/go/basic`.
- `annotations`: Zig `function` (`export`, `inline`, and `extern` linkage when
  present) and `variable` (`threadlocal`, `export`, `comptime`, and `align(...)`
  when present) in `fixtures/extraction/zig/basic`.

Scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 27
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 35/36 fixture-proven; 0 open doc-comment gaps because regex is `not_applicable`
- `structural_facts`: 12/36
- `complexity_metrics`: 28/36
- `annotations`: 21/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 9 Vue/Razor annotations and web structural facts

The next slices closed the final code-language annotation gaps and began the
structural-facts completion program with web-domain facts:

- `annotations`: Vue component/script-setup macro metadata and
  `defineExpose` API exposure facts now attach to owning Vue symbols.
- `annotations`: Razor embedded C# attributes now normalize on class, method,
  and property symbols.
- `structural_facts`: CSS now emits versioned facts for selector rules, custom
  properties, media queries, and keyframes.
- `structural_facts`: Vue now emits versioned facts for SFC sections and
  template directives.

Current scorecard:

- `silent_cells`: 0
- `quality_bar_debts`: 23
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 35/36 fixture-proven; 0 open doc-comment gaps because regex is `not_applicable`
- `structural_facts`: 14/36
- `complexity_metrics`: 28/36
- `annotations`: 23/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 10 data/document structural facts

The next slice closed the data and markup structural-facts debt for the
format languages that downstream tools query most often:

- `structural_facts`: Markdown now emits versioned facts for frontmatter,
  headings, fenced code blocks, link definitions, and pipe tables.
- `structural_facts`: JSON now emits object, array, and property facts with
  path, depth, and value-kind metadata while avoiding scalar noise.
- `structural_facts`: TOML now emits table, array-table, key-value, and
  inline-table facts with key paths and value kinds.
- `structural_facts`: YAML now emits document, mapping, sequence, anchor, and
  alias facts.
- `structural_facts`: Regex now emits capture groups, named captures,
  lookarounds, character classes, quantifiers, alternations, and anchors.
- `source_regions`: Regex is documented `not_applicable` for
  `comment`, `doc_comment`, `string_literal`, and `embedded` because the
  current source-region contract models host-language comments and literals,
  not regex-pattern internals.

Current scorecard:

- `silent_cells`: 0
- `quality_bar_debts`: 17
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36 fixture-proven; regex closed via capability
  `not_applicable` (no golden `source_regions` rows)
- `doc_comments`: 35/36 fixture-proven; regex `not_applicable`
- `structural_facts`: 19/36
- `complexity_metrics`: 28/36
- `annotations`: 23/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 11 SQL structural facts

The next slice closed SQL structural-facts debt with a focused collector
rather than generic node-kind matching:

- `structural_facts`: SQL now emits versioned facts for table/view/trigger/index
  definitions, columns, foreign keys, selects, CTEs, joins, transactions, and
  update statements.
- SQLite-style triggers that parse as `ERROR` nodes still emit
  `sql.trigger_definition.v1` via controlled fallback parsing.

Current scorecard:

- `silent_cells`: 0
- `quality_bar_debts`: 16
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36 fixture-proven; regex closed via capability
  `not_applicable` (no golden `source_regions` rows)
- `doc_comments`: 35/36 fixture-proven; regex `not_applicable`
- `structural_facts`: 20/36
- `complexity_metrics`: 28/36
- `annotations`: 23/36
- `literals`: 36/36
- `type_argument_usages`: 1/36

## Phase 12 JVM and mobile code structural facts

Task 10 slice closed structural-facts debt for Java, Kotlin, Scala, Swift,
Dart, and VB.NET via `base/code_structural_facts.rs`:

- **Java**: `java.synchronized_statement.v1`, `java.try_with_resources_statement.v1`,
  `java.lambda_expression.v1`, `java.marker_annotation.v1`, `java.annotation.v1`
  (parameterized annotations such as `@SuppressWarnings("unchecked")`).
- **Kotlin**: `kotlin.suspend_modifier.v1`, `kotlin.property_delegate.v1`,
  `kotlin.annotation.v1`.
- **Scala**: `scala.extension_definition.v1`, `scala.given_definition.v1`,
  `scala.for_expression.v1`, `scala.annotation.v1` (extension blocks without
  leading `@ops`; `@ops extension` parses as `function_definition` + ERROR).
- **Swift**: `swift.await_expression.v1`, `swift.actor_declaration.v1` (actor
  bodies are `class_declaration` nodes whose text starts with `actor`),
  `swift.attribute.v1`.
- **Dart**: `dart.await_expression.v1`, `dart.async_modifier.v1`,
  `dart.annotation.v1`.
- **VB.NET**: `vbnet.handles_clause.v1`, `vbnet.implements_clause.v1`,
  `vbnet.event_declaration.v1`, `vbnet.attribute.v1`.

Rejected candidates (no reliable tree-sitter node kinds): Kotlin coroutine
`launch`/`async`/`withContext` calls (generic `call_expression` only), VB.NET
async/await (no dedicated nodes in tree-sitter-vb-dotnet), Dart Flutter
widget/build hooks (ordinary `method_declaration` only).

Current scorecard:

- `silent_cells`: 0
- `quality_bar_debts`: 10
- `symbols`: 36/36
- `relationships`: 36/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `source_regions`: 35/36 fixture-proven; regex closed via capability
  `not_applicable` (no golden `source_regions` rows)
- `doc_comments`: 35/36 fixture-proven; regex `not_applicable`
- `structural_facts`: 26/36
- `complexity_metrics`: 28/36
- `annotations`: 23/36
- `literals`: 36/36
- `type_argument_usages`: 3/36

## Remaining verified gaps

### 1. Open domain gaps are now explicit product debt

No language/domain cells are silent anymore. The remaining debt is explicit
`open_gaps` in the capability matrix. Largest buckets:

- `complexity_metrics`: 8 languages remain open (HTML, CSS, SQL, JSON, TOML,
  YAML, Markdown, Regex).
- `structural_facts`: 10 languages remain open (zig, php, ruby, elixir, lua,
  qml, r, bash, powershell, gdscript).
- `annotations`: 0 code languages remain open; Vue and Razor closed in Task 5 with
  script-setup macro and embedded C# attribute fixture evidence.
- `literals`: 0 languages remain open.
- `doc_comments`: 35/36 fixture-proven; 0 open doc-comment gaps because regex is
  `not_applicable` (tree-sitter-regex has no comment or doc-comment nodes).

Impact: downstream consumers cannot distinguish "not applicable" from "not
implemented yet" or "not audited" unless the matrix stays fail-closed.
Open gaps are temporary debt, not acceptance criteria.

### 2. Literal extraction is under-advertised

There are per-language literal unit tests for many languages, including Rust,
C, C++, Go, Zig, Python, Java, VB.NET, PHP, Swift, Kotlin, Scala, Dart,
Elixir, QML, GDScript, Razor, and others. Golden fixtures and
`kind_coverage.literals` now advertises 36/36 languages with golden fixture
evidence across code, markup, query, and data formats.

Impact: literal extraction should be a standard domain for languages with
string, URL, query, command, or configuration literals. Existing unit tests show
that much of the implementation path exists, but the product contract and
goldens do not yet make it first-class across the matrix.

### 3. Complexity metrics remain uneven

Current fixture-proven languages:

`rust`, `c`, `cpp`, `go`, `zig`, `typescript`, `tsx`, `javascript`, `jsx`,
`python`, `java`, `csharp`, `vbnet`, `php`, `ruby`, `swift`, `kotlin`,
`scala`, `dart`, `elixir`, `lua`, `r`, `bash`, `powershell`, `gdscript`,
`qml`, `vue`, and `razor`.

Likely code-language targets still missing:

`sql` (design-gated separately).
SQL should be design-gated separately because procedural SQL complexity is not
the same metric as cyclomatic complexity in general-purpose code.

Several format or markup languages need a different semantic standard rather
than a lowered one: `json`, `toml`, `yaml`, `markdown`, `regex`, and `css`
should expose their own structure deeply, and only domains that genuinely do
not exist in the language should become `not_applicable`.

### 4. Identifier richness is weak in specific languages

Low current identifier variety:

- `jsx`: only `call`.
- `vue`: only `call`.
- `bash`: only `call`.
- `sql`: only `member_access`.
- `yaml`: only `variable_ref`.

Some of these may be legitimate format constraints, but Bash, JSX, Vue, YAML,
and SQL should be treated as extraction-quality defects until AST inspection
proves otherwise.

### 5. Doc-comment coverage improved but policy is still inconsistent

Golden evidence exists for 25 languages. Missing or unaudited languages include
`zig`, `tsx`, `vbnet`, `scala`, `elixir`, `lua`, `qml`, `r`, `gdscript`, plus
likely not-applicable rows for `regex` and `yaml`.

The bigger issue is policy: some languages store marker-stripped text while
others preserve raw comment markers. The next pass should add a shared
normalization policy and then update fixtures.

### 6. Annotation support is probably under-claimed

Current golden evidence exists for:

`rust`, `typescript`, `javascript`, `python`, `java`, `csharp`, `dart`,
`elixir`, `gdscript`.

Source search shows additional languages already call `normalize_annotations`
or have attribute helpers, including C++, PHP, VB.NET, PowerShell, and Scala.
Those need an audit to decide whether to add fixture/capability evidence or
record explicit gaps.

### 7. Structural facts need a higher product bar

Only 12 languages emit structural facts. The target is not "one structural fact
per language"; filler rows would be worse than no rows. The target is stronger:
every language family should be reviewed for high-value semantic constructs
that downstream tools would otherwise have to rediscover from raw source.

Candidate high-value facts to evaluate:

- JSX/TSX/Vue component boundaries and template/script/style relationships.
- PHP/Ruby/Java/Kotlin/Scala framework annotations or route declarations.
- SQL DDL/DML categories and procedural blocks.
- GDScript signals, exported variables, and scene/resource links.
- Swift/Kotlin/Java concurrency or async constructs.

### 8. Known language-specific defects remain

Phase 5 (2026-06-10) closed the three tracked defects below. Remaining SQL
recovery limitations are explicit via `extractedFromError` + `bodySpanSource`
metadata rather than silent weak spans.

- ~~Dart generic-modifier recovery path checks for `program`, but
  tree-sitter-dart uses `source_file`.~~ Fixed: guards use `source_file`; tests
  document active recovery vs clean parse.
- ~~C# return-type inference can be corrupted by substring matching when an
  attribute argument contains the method name.~~ Fixed: exact `MethodName(`
  matching with regression coverage.
- ~~SQL body-span quality remains weak for views/triggers that come from recovery
  paths.~~ Improved: statement-level body spans plus `bodySpanSource` metadata;
  golden fixture updated for SQL basic views and triggers.

## Phase 10 structural facts: PHP, Ruby, Elixir, Lua, R

Task 10 slice closed structural-facts debt for five scripting languages using
parser-backed collectors in `code_structural_facts.rs`, focused fixture sources,
golden rows, and per-language `structural_facts.rs` tests.

- PHP: attributes, namespace/use declarations, trait use, anonymous functions,
  match expressions.
- Ruby: `require` / `require_relative`, mixin calls (`include` / `extend` /
  `prepend`), blocks, rescue clauses.
- Elixir: `defmodule`, module attributes (`@spec`, `@doc`, `@moduledoc`), directive
  calls (`use`, `import`, `alias`, `require`), pipeline (`|>`), `with`.
- Lua: `require`, `setmetatable`, `coroutine.*` calls, chunk-level module
  returns, table constructors with field metadata.
- R: `library()` / `require()`, pipe (`|>`), formula (`~`) expressions.

Rejected candidates:

- PHP `try`/`catch`: not added to the basic fixture; match and anonymous
  functions cover control-flow and functional debt instead.
- Ruby metaprogramming (`define_method`, etc.): unstable as structural facts;
  blocks and mixin calls are parser-backed and fixture-proven.
- Elixir Phoenix/Rails-style framework facts: not statically visible in fixture
  syntax.
- Lua bare `return` inside functions: filtered out; only chunk-level module
  exports emit `lua.module_return.v1`.
- R S3/S4/R6 class declarations: `R6::R6Class(...)` is a generic call without
  reliable static class-shape metadata beyond existing symbol extraction.

Current scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 5
- `structural_facts`: 31/36

Remaining structural-facts debt: `zig`, `qml`, `bash`, `powershell`, `gdscript`.

## Phase 11 structural facts: Zig, QML, Bash, PowerShell, GDScript

Task 11 slice closed the remaining structural-facts debt for five languages
using parser-backed collectors in `code_structural_facts.rs`, focused fixture
sources, golden rows, and per-language `structural_facts.rs` tests.

- Zig: `@import` / other builtin calls, `threadlocal var`, `inline fn`,
  `export fn`, `comptime` parameters.
- QML: `import`, `property`, `signal`, and property-binding declarations.
- Bash: shebang, command substitution, arithmetic expansion, `export`
  declarations.
- PowerShell: `[CmdletBinding()]`, `param(...)` blocks, pipeline expressions,
  class definitions.
- GDScript: `class_name`, `extends`, `signal`, `@export`, and `match`
  statements.

Rejected candidates:

- QML JavaScript statement bodies: not emitted as structural facts; QML UI
  semantics only.
- Bash plain command invocations: filtered out of command-substitution facts.
- PowerShell typed parameter attributes such as `[int]`: not confused with
  `CmdletBinding`.
- GDScript duplicate facts from nested broad matches: guarded with focused
  `matches_pattern` checks.

Parser-shape decisions:

- Zig `@import("std")` parses as `builtin_function`; metadata normalizes
  `@`-prefixed builtin identifiers to bare names (`import`).
- GDScript `@export var` uses an `annotation` child under `variable_statement`,
  not a standalone `export_variable_statement` node.
- PowerShell class names come from the first direct `simple_name` child of
  `class_statement`, not the first type annotation inside the body.

Current scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 0
- `structural_facts`: 36/36

Remaining structural-facts debt: none.

## Phase 12 type-argument usage: Rust, C#, Java, Kotlin, Swift, VB.NET, PowerShell, Razor, Vue

Task 12 slice promoted generic-capable languages from shallow fixture evidence
to product-grade `type_argument_usages` coverage using nested generic use sites
in basic fixtures, golden rows, and `extract_canonical` fixture tests.

- Rust: `HashMap<String, Vec<u8>>` return type on `build_index()`.
- C#: `Dictionary<string, List<int>>` field on `ComplexityFixture`.
- Java: `Map<String, List<Integer>>` field on `Worker`.
- Kotlin: `List<Map<String, Int>>` property on `Worker`.
- Swift: `Array<Dictionary<String, Int>>` property on `Worker`.
- VB.NET: `Dictionary(Of String, List(Of Integer))` field on `Worker`.
- PowerShell: `[Dictionary[string, List[int]]]$script:WorkerIndex`.
- Razor: `Dictionary<string, List<int>>` field in `@code`.
- Vue: `Map<string, Array<number>>` in `<script setup lang="ts">`.

Each language now emits exactly one golden `type_argument_usages` row with
ordered top-level arguments and nested children where applicable. Focused
tests assert the basic fixture through `extract_canonical(...)` plus a
negative guard that plain non-generic symbols in the same fixture do not emit
rows.

No extractor implementation changes were required; existing per-language
type-argument collectors already supported the chosen patterns.

Current scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 0
- `type_argument_usages`: 12/36

Languages with fixture-proven type-argument usage evidence:

`rust`, `typescript`, `vue`, `java`, `csharp`, `vbnet`, `swift`, `kotlin`,
`scala`, `dart`, `powershell`, `razor`.

Remaining type-argument usage debt: 24 languages without golden rows.

## Phase 13 type-argument usage: C++, Go, Zig, Python, QML, TSX

Task 13 slice promoted six additional generic-capable languages from shallow
fixture evidence to product-grade `type_argument_usages` coverage using nested
generic use sites in basic fixtures, golden rows, and `extract_canonical`
fixture tests.

- C++: `Map<int, Vec<Item>>` global on `worker_index`.
- Go: `Map[string, List[int]]` package variable on `workerIndex`.
- Zig: `Map(Key, ArrayList(User))` type-position variable on `workerIndex`.
- Python: `Dict[str, List[int]]` module variable on `worker_index` (typing
  names; builtin `dict`/`list` are intentionally skipped by the extractor).
- QML: `Map<string, Array<User>>` typed parameter on `buildIndex()`.
- TSX: `Map<string, Array<number>>` variable on `workerIndex` in the basic
  fixture; proven through `extract_canonical(...)` with the TSX grammar, not
  the plain `.ts` parser helper.

Each language now emits exactly one golden `type_argument_usages` row with
ordered top-level arguments and nested children where applicable. Focused
tests assert the basic fixture through `extract_canonical(...)` plus a
negative guard that plain non-generic symbols in the same fixture do not emit
rows.

No extractor implementation changes were required; existing per-language
type-argument collectors already supported the chosen patterns.

Current scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 0
- `type_argument_usages`: 18/36

Languages with fixture-proven type-argument usage evidence:

`rust`, `typescript`, `vue`, `java`, `csharp`, `vbnet`, `swift`, `kotlin`,
`scala`, `dart`, `powershell`, `razor`, `cpp`, `go`, `zig`, `python`, `qml`,
`tsx`.

Remaining type-argument usage debt: 18 languages without golden rows.

## Phase 14 type-argument usage: GDScript

Task 14 slice promoted GDScript from unit-test-only evidence to golden
fixture-proven `type_argument_usages` coverage:

- GDScript: `var worker_index: Array[Array[int]]` in
  `fixtures/extraction/gdscript/basic/source.gd`.
- Exactly one golden row for the outermost `Array` use site with nested
  `Array[int]` child at ordinal 0.
- Canonical-pipeline tests in
  `crates/julie-extractors/src/tests/gdscript/type_arguments.rs` assert the
  basic fixture through `extract_canonical(...)` plus a negative guard that
  plain `@export var id: int` does not emit rows.

No extractor implementation changes were required; existing GDScript
type-argument collectors already supported `Array[Array[int]]` bracket syntax.

Current scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 0
- `type_argument_usages`: 19/36

Languages with fixture-proven type-argument usage evidence:

`rust`, `typescript`, `vue`, `java`, `csharp`, `vbnet`, `swift`, `kotlin`,
`scala`, `dart`, `powershell`, `razor`, `cpp`, `go`, `zig`, `python`, `qml`,
`tsx`, `gdscript`.

Remaining type-argument usage debt: 17 languages without golden rows.

## Phase 14 type-argument applicability audit (remaining 16 languages)

Repo inspection of extractors, fixtures, and per-language tests for the
languages still without golden `type_argument_usages` rows after Phase 15
closed Elixir native typespec debt. Goal: distinguish implementation debt
from constructs that genuinely do not exist in the language grammar.

| Language | Classification | Evidence inspected |
| --- | --- | --- |
| `c` | `not_applicable` | `c/mod.rs` delegates empty `get_type_argument_usages`; `c/signatures.rs` only mentions macro generics; C++ debt is `template_type` in `cpp/identifiers.rs`, not C. |
| `javascript` | `not_applicable` | `fixtures/extraction/javascript/basic/source.js` has no typed generics; TypeScript/TSX golden rows already cover typed ECMAScript. |
| `jsx` | `not_applicable` | `fixtures/extraction/jsx/basic/source.jsx` is untyped JSX/JS; TSX fixture proves generic markup scripts separately. |
| `html` | `not_applicable` | `fixtures/extraction/html/basic/source.html` has element/attribute markup only; `html/mod.rs` delegates empty `get_type_argument_usages`; `html/attributes.rs` parses attribute strings, not generic type applications; `html/basic/expected.json` has empty `type_argument_usages`. |
| `css` | `not_applicable` | `fixtures/extraction/css/basic/source.css` has selectors/properties/at-rules only; `css/mod.rs` delegates empty `get_type_argument_usages`; `css/rules.rs` extracts selector/declaration blocks, not generic type syntax; `css/basic/expected.json` has empty `type_argument_usages`. |
| `php` | `convention_only` | `fixtures/extraction/php/basic/source.php` uses native `int` only; `tests/php/mod.rs` PHPDoc shows `array<string,mixed>` / `Collection<User>` in comments, not PHP declaration syntax. |
| `ruby` | `convention_only` | `fixtures/extraction/ruby/basic/source.rb` has no native generics; `ruby/symbols.rs` extracts RDoc/YARD doc comments only. |
| `lua` | `convention_only` | `fixtures/extraction/lua/basic/source.lua` has no typed declarations; `lua/identifiers.rs` defers type usage and documents no LuaLS `---@` annotation extractor. |
| `r` | `not_applicable` | `tests/r/classes.rs` S4 “generic” is runtime OOP metadata (`s4_generic`), not a static type-argument syntax; `r/basic` golden has empty `type_argument_usages`. |
| `bash` | `not_applicable` | `fixtures/extraction/bash/basic` has shell commands only; no typed generic construct in bash extractor modules. |
| `sql` | `not_applicable` | `fixtures/extraction/sql/basic/source.sql` is DDL/DML without parameterized type syntax; `sql/mod.rs` has no type-argument collector beyond base empty delegate. |
| `regex` | `not_applicable` | Pattern quantifiers/groups are structural facts (`regex/basic` golden), not generic type applications. |
| `markdown` | `not_applicable` | Data/document family; Phase 10 structural facts only, no type syntax in `markdown/basic`. |
| `json` | `not_applicable` | Scalar/object/array values only in `json/basic`; no generic type construct. |
| `toml` | `not_applicable` | Table/key-value schema only in `toml/basic`; no generic type construct. |
| `yaml` | `not_applicable` | Mapping/sequence schema only in `yaml/basic`; no generic type construct. |

Summary:

- `native_applicability_missing`: 0.
- `convention_only`: 3 (`php` PHPDoc, `ruby` RDoc/YARD, `lua` deferred LuaLS-style annotations).
- `not_applicable`: 13 (C, untyped JS/JSX, web/data formats, bash, sql, regex, r).
- `needs_followup`: 0.

Convention-only languages still need an explicit product decision before golden padding.

## Phase 15 type-argument usage: Elixir

Task 15 slice promoted Elixir from `native_applicability_missing` to golden
fixture-proven `type_argument_usages` coverage:

- Elixir: `@type worker_index :: list(list(integer()))` in
  `fixtures/extraction/elixir/basic/source.ex`.
- Exactly one golden row for the outermost `list` use site with nested
  `list(integer())` child at ordinal 0 (`integer` leaf).
- Extractor change in `elixir/identifiers.rs`: walk `@type` / `@typep` /
  `@opaque` / `@spec` / `@callback` attribute trees, record parameterized
  typespec `call` nodes (e.g. `list(...)`) only in typespec contexts, skip
  `@spec` function heads and zero-argument primitives like `integer()`.
- Canonical-pipeline tests in
  `crates/julie-extractors/src/tests/elixir/type_arguments.rs` assert the
  basic fixture through `extract_canonical(...)` plus negative guards for
  `@spec run(integer())` and runtime calls.

Current scorecard after this slice:

- `silent_cells`: 0
- `quality_bar_debts`: 0
- `type_argument_usages`: 20/36

Languages with fixture-proven type-argument usage evidence:

`rust`, `typescript`, `vue`, `java`, `csharp`, `vbnet`, `swift`, `kotlin`,
`scala`, `dart`, `powershell`, `razor`, `cpp`, `go`, `zig`, `python`, `qml`,
`tsx`, `gdscript`, `elixir`.

Remaining type-argument usage debt: 16 languages without golden rows.

## Phase 16 scorecard v2 (applicability-aware view)

`scripts/language-data-quality-report.mjs` now prints the existing raw
`Fixture-Proven Domain Counts` section unchanged, then an
`Applicability-Aware Domain View` that classifies each observed domain beyond
raw `N/36` fixture rows.

Current behavior:

- Script-local `DOMAIN_APPLICABILITY` metadata is validated before printing:
  unknown language names, duplicate bucket assignments, and fixture-proven
  conflicts for `not_applicable`, `convention_only`, or `native_debt` throw with
  domain, bucket, and language; `quality_debt` may coexist with fixture rows.
- Every `OBSERVED_DOMAINS` entry reports `applicable_closure`,
  `fixture_proven_native`, optional applicability buckets, and
  `unclassified_gaps`.
- Domains without script-local applicability metadata do not invent
  `not_applicable` or `convention_only` rows; languages lacking golden evidence
  appear only under `unclassified_gaps`.
- `type_argument_usages` is the first fully classified domain via script-local
  metadata aligned with the Phase 14 audit:

| Bucket | Count | Languages |
| --- | ---: | --- |
| fixture-proven native | 20 | `rust`, `cpp`, `go`, `zig`, `typescript`, `tsx`, `vue`, `python`, `java`, `csharp`, `vbnet`, `swift`, `kotlin`, `scala`, `dart`, `elixir`, `qml`, `powershell`, `gdscript`, `razor` |
| `not_applicable` | 13 | `c`, `javascript`, `jsx`, `html`, `css`, `r`, `bash`, `sql`, `regex`, `markdown`, `json`, `toml`, `yaml` |
| `convention_only` | 3 | `php`, `ruby`, `lua` |
| `native_debt` | 0 | none |
| `quality_debt` | 0 | none |
| `unclassified_gaps` | 0 | none |

Interpretation: the raw scorecard still reads `type_argument_usages: 20/36`,
but applicability closure is `20/20 complete` because the remaining 16
languages are accounted for as true non-applicability (13) or convention-only
documentation idioms (3), not extractor debt. Strict mode still reports
`quality_bar_debts` in the header, but exit nonzero remains the pre-existing
silent-cell gate on `kind_coverage` only; convention-only or not-applicable
type-argument rows do not affect strict exit.

## Phase 17 raw-gap applicability audit (six domains)

Repo inspection of golden fixtures, `fixtures/extraction/capabilities.json`
capability gaps, and per-language tests for every language still appearing
under scorecard v2 `unclassified_gaps` in `relationships`, `identifiers`,
`body_spans`, `source_regions`, `pending_relationships`, and `types`.

| Domain | Language | Classification | Evidence inspected |
| --- | --- | --- | --- |
| `relationships` | `ruby` | `native_debt` | `fixtures/extraction/ruby/basic/expected.json` and `cross_file/expected.json` have empty `relationships` while `pending_relationships` is populated; `ruby/relationships.rs` emits resolved inheritance/module/call edges; `tests/ruby/mod.rs::test_extract_inheritance_and_module_relationships` and `tests/ruby/cross_file_relationships.rs::test_same_file_method_call_creates_relationship` expect resolved same-file edges. |
| `identifiers` | `json` | `not_applicable` | `capabilities.json` `capability_gaps.identifiers` and `kind_coverage.identifiers.not_applicable`; `fixtures/extraction/json/basic/expected.json` has empty `identifiers`; object keys are symbols/relationships. |
| `identifiers` | `markdown` | `not_applicable` | `capabilities.json` `capability_gaps.identifiers`; `fixtures/extraction/markdown/basic/expected.json` has empty `identifiers`; headings/links are symbols/relationships. |
| `identifiers` | `toml` | `not_applicable` | `capabilities.json` `capability_gaps.identifiers`; `fixtures/extraction/toml/basic/expected.json` has empty `identifiers`; tables/keys are symbols/relationships. |
| `body_spans` | `yaml` | `not_applicable` | `capabilities.json` `kind_coverage.body_spans.not_applicable` for `module` and `variable`; `fixtures/extraction/yaml/basic/expected.json` symbols have `body_span: null`; YAML mappings/sequences have no callable bodies. |
| `source_regions` | `regex` | `not_applicable` | `capabilities.json` `kind_coverage.source_regions.not_applicable` for `comment`, `doc_comment`, `embedded`, `string_literal`; `fixtures/extraction/regex/basic/expected.json` has empty `source_regions`; Phase 10 findings; tree-sitter-regex has no comment/doc-comment nodes and the source-region contract models host-language regions, not pattern internals. |
| `pending_relationships` | `css` | `not_applicable` | `capabilities.json` `capability_gaps.pending_relationships`; `tests/css/cross_file_pending::css_pending_relationships_intra_document_only`; `@import` resolves at extraction time. |
| `pending_relationships` | `markdown` | `not_applicable` | `capabilities.json` `capability_gaps.pending_relationships`; `tests/markdown/cross_file_pending::markdown_pending_relationships_intra_document_only`; links are URL/path strings, not deferred symbol refs. |
| `pending_relationships` | `razor` | `not_applicable` | `capabilities.json` `capability_gaps.pending_relationships`; `tests/razor/cross_file_pending::razor_pending_relationships_handled_by_csharp_embed`; cross-file refs resolve through embedded C#. |
| `pending_relationships` | `regex` | `not_applicable` | `capabilities.json` `capability_gaps.pending_relationships`; `tests/regex/cross_file_pending::regex_pending_relationships_within_pattern_only`; backreferences are within-pattern only. |
| `pending_relationships` | `toml` | `not_applicable` | `capabilities.json` `capability_gaps.pending_relationships`; TOML references are file-local (Cargo deps, pyproject tables). |
| `pending_relationships` | `yaml` | `not_applicable` | `capabilities.json` `capability_gaps.pending_relationships`; `tests/yaml/cross_file_pending::yaml_pending_relationships_intra_document_only`; anchors/aliases are within-document. |
| `types` | `css` | `not_applicable` | `capabilities.json` `capability_gaps.types`; no static type system; custom properties are runtime strings. |
| `types` | `json` | `not_applicable` | `capabilities.json` `capability_gaps.types`; no static type system; JSON Schema type keywords are out of tree-sitter scope. |
| `types` | `lua` | `convention_only` | Dynamically typed runtime language; `fixtures/extraction/lua/basic/expected.json` has empty `types` and inferred `dataType` lives on symbol metadata; LuaLS `---@type` / `---@param` annotations are documentation conventions, not native TypeInfo syntax; product decision whether to harvest them. |
| `types` | `markdown` | `not_applicable` | `capabilities.json` `capability_gaps.types`; presentation format with no static type system. |
| `types` | `qml` | `native_debt` | `fixtures/extraction/qml/basic/source.qml` has native property types (`property string title`, `property int workerId`); `qml/semantics.rs` implements `infer_types`, `infer_property_type_from_signature`, and `infer_function_return_type_from_signature`; `tests/qml/coverage.rs` asserts inferred property/function types; `fixtures/extraction/qml/basic/expected.json` has empty `types`; inference exists but product `TypeInfo` rows are not emitted yet. |
| `types` | `r` | `convention_only` | Dynamically typed runtime language; S3/S4/R6 class systems and roxygen `@param` tags are type-like documentation/runtime conventions, not stable native TypeInfo syntax; `fixtures/extraction/r/basic/expected.json` has empty `types`. |
| `types` | `toml` | `not_applicable` | `capabilities.json` `capability_gaps.types`; value kinds are format-level, not TypeInfo. |
| `types` | `yaml` | `not_applicable` | `capabilities.json` `capability_gaps.types`; tag types (`!!str`, `!!int`) are format-level, not TypeInfo. |

Summary:

- `not_applicable`: 16 language/domain pairs across five domains (all gaps except `relationships`, `types.qml`, and `types.lua`/`types.r`).
- `convention_only`: 2 in this audit slice (`lua` `types`, `r` `types`).
- `native_debt`: 2 (`ruby` `relationships`, `qml` `types`).
- `quality_debt`: 0.
- `unclassified_gaps`: 0 for the six audited domains after script-local metadata update.

Product-decision follow-ups for `convention_only` (`types`):

- **Lua types:** Decide whether LuaLS-style `---@type` / `---@param` annotations or
  existing inferred `dataType` symbol metadata should be promoted into `TypeInfo`
  rows; no native static type syntax exists today.
- **R types:** Decide whether roxygen `@param` tags or S3/S4/R6 runtime class
  metadata should be harvested into `TypeInfo` rows; no stable native TypeInfo
  syntax exists today.

Closure tasks for `native_debt`:

- **Ruby relationships:** Promote resolved same-file inheritance, module inclusion,
  and call edges from the Ruby relationship collector into golden `relationships`
  rows (today they appear only under `pending_relationships` in
  `fixtures/extraction/ruby/basic/expected.json`). Align canonical extraction with
  `tests/ruby/cross_file_relationships.rs::test_same_file_method_call_creates_relationship`
  and `tests/ruby/mod.rs::test_extract_inheritance_and_module_relationships`, then
  regenerate Ruby goldens and verify `relationships` reaches fixture-proven closure.
- **QML types:** Promote QML inferred property and function return types from
  `qml/semantics.rs` into product `TypeInfo` rows and golden `types` output for
  native declarations such as `property string title` and `property int workerId`
  in `fixtures/extraction/qml/basic/source.qml`. Extend
  `tests/qml/coverage.rs` and regenerate QML goldens so inferred types are
  fixture-proven, not only symbol metadata.

## Phase 18 native-debt closure (Ruby relationships, QML types)

**Before:** scorecard v2 listed `relationships.ruby` and `types.qml` as
`native_debt` with raw counts `relationships 35/36` and `types 28/36`. Ruby
resolved call edges were blocked when a `require_relative` import symbol shared
the callee name with an in-class method (`helper`). QML `infer_types` existed
but `extract_qml` returned an empty `types` map.

**After:** `relationships 36/36`, `types 29/36`, and both domains report
`native_debt: 0` in `scripts/language-data-quality-report.mjs --strict`.

| Change | Evidence |
| --- | --- |
| Ruby scoped call resolution prefers same-parent methods over import-name collisions | `ruby/relationships.rs::resolve_ruby_call_target`; `tests/ruby/canonical_relationships.rs` |
| Ruby basic golden `relationships` | `run` -> `helper` `calls` edge at line 14; unresolved `Enumerable` / cross-file calls remain in `pending_relationships` |
| QML product `TypeInfo` rows | `registry.rs::extract_qml` now uses `convert_types_map(ext.infer_types(&symbols), "qml")` |
| QML basic golden `types` | `title` -> `string`, `workerId` -> `int`, `buildIndex` -> `void` |
| Caller-facing QML types test | `tests/qml/types.rs::canonical_qml_extraction_emits_property_and_function_types` via `extract_symbols_and_relationships` |
| Capabilities | `fixtures/extraction/capabilities.json` sets QML `types: true` and clears the prior types exception gap |

**Verification commands:**

```bash
cargo xtask test language ruby
cargo xtask test language qml
UPDATE_GOLDEN=1 cargo nextest run -p julie-extractors --features test-golden golden
cargo nextest run -p julie-extractors --features test-golden golden
cargo test -p julie-extractors --features test-capability-matrix tests::capability_matrix::
node scripts/language-data-quality-report.mjs --strict
git diff --check
cargo fmt --check
```

Remaining scorecard gaps in these domains are classified (`not_applicable` or
`convention_only` for `types.lua` / `types.r`), not unclosed native debt.

## Phase 19 applicability closure (complexity_metrics, annotations, doc_comments)

**Before:** strict scorecard listed 22 unclassified applicability gaps across
`complexity_metrics` (8), `annotations` (13), and `doc_comments` (1).

**After:** all three domains report `unclassified_gaps: 0` in
`scripts/language-data-quality-report.mjs --strict`.

| Domain | Language | Decision | Evidence |
| --- | --- | --- | --- |
| `complexity_metrics` | `sql` | native extraction | `sql/complexity_metrics.rs` emits `julie-sql-complexity-v1` file/symbol metrics counting joins, predicates (`where`/`having`), set operations, `case` arms, and query nesting (`select`/`cte`); `tests/sql/complexity.rs`; golden `fixtures/extraction/sql/basic/expected.json`. |
| `complexity_metrics` | `regex` | native extraction | `regex/complexity_metrics.rs` emits `julie-regex-complexity-v1` metrics counting alternations/conditionals, quantifiers, and group/lookaround nesting; `tests/regex/complexity.rs`; golden `fixtures/extraction/regex/basic/expected.json`. |
| `complexity_metrics` | `css` | `not_applicable` | Declarative stylesheet language with selector/property structure but no callable control-flow bodies; complexity belongs in structural facts (`css.*.v1`), not cyclomatic-style metrics. |
| `complexity_metrics` | `html` | `not_applicable` | Markup language with element/attribute structure and embedded scripts handled by other language targets; no native file/symbol complexity model in HTML grammar. |
| `complexity_metrics` | `json` | `not_applicable` | Pure data format with object/array/value nodes and no control flow. |
| `complexity_metrics` | `markdown` | `not_applicable` | Presentation format with headings/links/tables; no callable bodies or control flow. |
| `complexity_metrics` | `toml` | `not_applicable` | Configuration tables/keys only; no control flow. |
| `complexity_metrics` | `yaml` | `not_applicable` | Document/mapping structure only; no control flow. |
| `annotations` | `bash` | `not_applicable` | `fixtures/extraction/capabilities.json` Phase 1 audit; shell has no attribute/decorator syntax on declarations (shebang and magic comments are source-region/doc conventions). |
| `annotations` | `css` | `not_applicable` | HTML/CSS/SQL/markup attributes are structural facts or symbol metadata, not symbol-attached annotation markers. |
| `annotations` | `html` | `not_applicable` | Element attributes (`class`, `hx-*`, `x-*`) are structural facts, not annotations domain. |
| `annotations` | `json` | `not_applicable` | Keys and values only; no annotation syntax. |
| `annotations` | `lua` | `not_applicable` | No first-class attribute syntax; EmmyLua `---@` tags are comment conventions already routed through `doc_comments`, consistent with Phase 1 Task 5. |
| `annotations` | `markdown` | `not_applicable` | No declaration-attached annotation syntax. |
| `annotations` | `qml` | `not_applicable` | QML property modifiers (`readonly`, `required`) are declaration keywords, not separate annotation markers; `capabilities.json` already marks all symbol kinds `not_applicable`. |
| `annotations` | `r` | `not_applicable` | No decorator syntax; roxygen `@tags` are doc-comment conventions (`doc_comments` domain). |
| `annotations` | `regex` | `not_applicable` | Pattern syntax has no symbol-attached annotation markers. |
| `annotations` | `ruby` | `not_applicable` | No native decorator/attribute syntax; YARD/RDoc tags are doc-comment conventions. |
| `annotations` | `sql` | `not_applicable` | `COMMENT ON` and dialect hints are metadata/comments, not declaration annotations. |
| `annotations` | `toml` | `not_applicable` | Tables and keys only. |
| `annotations` | `yaml` | `not_applicable` | Mappings/tags only. |
| `doc_comments` | `regex` | `not_applicable` | Inline `(?#...)` comments are dialect-specific and not reliably grammar-attached to symbols; extended-mode `#` comments require `(?x)` and are not stable doc regions. `source_regions` and `doc_comments` are already `not_applicable` in `capabilities.json`; `tests/regex/mod.rs` doc-comment tests document optional behavior without golden evidence. |

Summary:

- `native extraction`: 2 (`sql` `complexity_metrics`, `regex` `complexity_metrics`).
- `not_applicable`: 20 language/domain pairs.
- `convention_only`: 0 in this slice (annotation comment conventions intentionally stay in `doc_comments` / `not_applicable`, not `convention_only` annotations).
- `quality_debt`: 0.

**Verification commands:**

```bash
cargo xtask test language sql
cargo xtask test language regex
UPDATE_GOLDEN=1 cargo nextest run -p julie-extractors --features test-golden golden
cargo nextest run -p julie-extractors --features test-golden golden
cargo test -p julie-extractors --features test-capability-matrix tests::capability_matrix::
node scripts/language-data-quality-report.mjs --strict
git diff --check
cargo fmt --check
```

## Product bar

The desired end state is the best tree-sitter extraction product available for
the supported language set. That means:

- code languages should converge toward rich symbols, body spans, signatures,
  visibility, doc comments, relationships, identifiers, type data, literals,
  source regions, complexity metrics, annotations, and structural facts where
  the language has those constructs;
- data, markup, query, and domain languages should expose their own semantics
  deeply instead of being treated as lower-value skeletons;
- `not_applicable` should mean the construct genuinely does not exist in the
  language, not that the extractor has not implemented it yet;
- `open_gaps` should be temporary implementation debt with a named closure
  task;
- downstream project scans should prove the improvements outside synthetic
  fixtures.

## Phase 20 semantic-depth batch (data/markup/domain languages)

**Before:** HTML and Razor golden `structural_facts` were empty despite rich
symbol extraction. YAML mappings only exposed `pair_count` without stable key
paths. Markdown inline links were symbol-proven via line-regex fallback but had
no structural-fact rows. Several markup/config languages looked complete in
the scorecard while downstream tools still had to reparse source for routes,
links, and config key paths.

**After:** Five concrete, parser-backed structural-fact improvements with
metadata assertions and golden updates. Strict scorecard remains
`silent_cells: 0`, `quality_bar_debts: 0`.

| Language | Domain | Improvement | Downstream value |
| --- | --- | --- | --- |
| `html` | `structural_facts` | `html.link.v1`, `html.script.v1`, `html.form.v1`, `html.form_control.v1` from tree-sitter `element` / `script_element` nodes | Direct `href`/`src`/`type`/`id`/`name` routing without reparsing tag text |
| `razor` | `structural_facts` | `razor.page_directive.v1`, `razor.code_block.v1`, `razor.template_expression.v1` from razor grammar nodes | Route and template-expression facts for Blazor/MVC page analysis |
| `yaml` | `structural_facts` | `yaml.key_value.v1` with `key_path`, `value_kind`; mapping facts now carry parent `key_path` | Config path queries (`$.worker.id`) without walking raw YAML |
| `markdown` | `structural_facts` | `markdown.inline_link.v1` from AST nodes plus regex fallback aligned with symbol extraction | Inline link label/destination facts for doc navigation graphs |
| `sql`, `toml`, `json`, `css`, `regex`, `vue` | audited | Existing facts already expose query/schema/selector/pattern metadata with focused tests; no filler rows added in this slice | N/A |

**Tests added/extended:**

- `tests/html/structural_facts.rs`
- `tests/razor/structural_facts.rs`
- `tests/yaml/structural_facts.rs` (`key_path`, `value_kind`)
- `tests/markdown/structural_facts.rs` (`markdown.inline_link.v1`)

**Golden fixtures updated:** `html`, `razor`, `yaml`, `markdown` (`basic`,
`cross_file`, and dedicated `structural_facts` fixtures where present).

**Verification commands:**

```bash
cargo xtask test language html
cargo xtask test language razor
cargo xtask test language yaml
cargo xtask test language markdown
UPDATE_GOLDEN=1 cargo nextest run -p julie-extractors --features test-golden golden
cargo nextest run -p julie-extractors --features test-golden golden
node scripts/language-data-quality-report.mjs --strict
git diff --check
cargo fmt --check
```

**Remaining quality_debt / intentional non-changes:**

- `html.form.v1` is proven by the dedicated structural-facts fixture; the basic
  HTML fixture still proves link/script/form-control facts.
- `razor` template-expression facts skip empty expressions; invocation-shaped
  expressions are still emitted when the grammar provides a span.
- `yaml` `key_value` rows use `$.`-prefixed paths consistent with JSON
  property paths; flow-style mappings outside `block_mapping_pair` remain
  mapping-level facts only.
- `markdown` inline-link regex fallback mirrors symbol extraction; image links
  (`![alt](url)`) are intentionally excluded.
- No changes to SQL/TOML/JSON/CSS/regex/vue literals, `source_regions`, or
  `complexity_metrics` in this slice; those domains were audited and already
  carried downstream-useful metadata.

## Recommended next plan

Build the next pass around a high-bar language-quality program:

1. Add a scorecard and no-silent-cell policy so gaps are visible.
2. Raise broad domains like literals, doc comments, annotations, complexity,
   identifiers, and type-argument usage to product-grade coverage across all
   languages that can support them.
3. Define language-family structural facts with clear downstream value.
4. Fix the known Dart, C#, and SQL quality defects as test-first tasks.
5. Dogfood the result against dependent projects and representative real-world
   corpora, recording domain counts and regressions.
