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
- Complexity metrics expanded from 7 languages to 12.
- Annotation evidence expanded to 11 languages.
- Doc-comment evidence expanded to 25 languages.
- Source-region evidence expanded to 35 languages.
- Literal evidence expanded to 22 languages.
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
| source_regions | 35/36 | Regex is the only current miss. |
| doc_comments | 25/36 | Stronger, but normalization is still inconsistent. |
| structural_facts | 12/36 | Still concentrated in Tier-1 and recent web/framework work. |
| complexity_metrics | 12/36 | Mainstream languages improved; many code languages remain empty. |
| annotations | 11/36 | Attribute/decorator support remains patchy outside the first pass. |
| literals | 22/36 | Unit tests exist for many more languages than goldens advertise. |
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
- `complexity_metrics`: 12/36
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

## Remaining verified gaps

### 1. Open domain gaps are now explicit product debt

No language/domain cells are silent anymore. The remaining debt is explicit
`open_gaps` in the capability matrix. Largest buckets:

- `complexity_metrics`: 24 languages remain open.
- `structural_facts`: 24 languages remain open.
- `annotations`: 25 languages remain open.
- `literals`: 14 languages remain open.
- `doc_comments`: 11 languages remain open.

Impact: downstream consumers cannot distinguish "not applicable" from "not
implemented yet" or "not audited" unless the matrix stays fail-closed.
Open gaps are temporary debt, not acceptance criteria.

### 2. Literal extraction is under-advertised

There are per-language literal unit tests for many languages, including Rust,
C, C++, Go, Zig, Python, Java, VB.NET, PHP, Swift, Kotlin, Scala, Dart,
Elixir, QML, GDScript, Razor, and others. Golden fixtures and
`kind_coverage.literals`, however, currently advertise 22 languages:

`rust`, `c`, `cpp`, `go`, `typescript`, `javascript`, `vue`, `csharp`, `ruby`,
`lua`, `r`, `bash`, `powershell`, `python`, `java`, `php`, `swift`, `vbnet`,
`kotlin`, `scala`, `dart`, `elixir`.

Impact: literal extraction should be a standard domain for languages with
string, URL, query, command, or configuration literals. Existing unit tests show
that much of the implementation path exists, but the product contract and
goldens do not yet make it first-class across the matrix.

### 3. Complexity metrics remain uneven

Current fixture-proven languages:

`rust`, `c`, `cpp`, `go`, `typescript`, `javascript`, `python`, `java`,
`csharp`, `swift`, `kotlin`, `dart`.

Likely code-language targets still missing:

`zig`, `tsx`, `jsx`, `vue`, `vbnet`, `php`, `ruby`, `scala`, `elixir`,
`lua`, `qml`, `r`, `bash`, `powershell`, `gdscript`, and `razor`.
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

Keep these as explicit implementation tasks, not buried in broad coverage work:

- Dart generic-modifier recovery path checks for `program`, but
  tree-sitter-dart uses `source_file`.
- C# return-type inference can be corrupted by substring matching when an
  attribute argument contains the method name.
- SQL body-span quality remains weak for views/triggers that come from recovery
  paths.

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
