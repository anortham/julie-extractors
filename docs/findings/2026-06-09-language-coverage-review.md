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
- Annotation evidence expanded to 9 languages.
- Doc-comment evidence expanded to 23 languages.
- Source-region evidence expanded to 35 languages.
- No current `kind_coverage` positive claim lacks fixture evidence.

This means the remaining work is not a stale-declaration cleanup. The new gap
is target quality: many languages still have deliberately empty domains or
thin evidence compared with the richer languages.

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
| doc_comments | 23/36 | Stronger, but normalization is still inconsistent. |
| structural_facts | 12/36 | Still concentrated in Tier-1 and recent web/framework work. |
| complexity_metrics | 12/36 | Mainstream languages improved; many code languages remain empty. |
| annotations | 9/36 | Attribute/decorator support remains patchy outside the first pass. |
| literals | 9/36 | Unit tests exist for many more languages than goldens advertise. |
| type_argument_usages | 1/36 | Type-argument usage evidence is TypeScript-only in goldens. |

## Remaining verified gaps

### 1. Empty domain cells are silent

Many language/domain cells have empty `supported`, empty `not_applicable`, and
empty `open_gaps`. Examples:

- `complexity_metrics`: 24 languages silent.
- `structural_facts`: 24 languages silent.
- `annotations`: 27 languages silent.
- `literals`: 27 languages silent.
- `doc_comments`: 13 languages silent.

Impact: downstream consumers cannot distinguish "not applicable" from "not
implemented yet" or "not audited."

### 2. Literal extraction is under-advertised

There are per-language literal unit tests for many languages, including Rust,
C, C++, Go, Zig, Python, Java, VB.NET, PHP, Swift, Kotlin, Scala, Dart,
Elixir, QML, GDScript, Razor, and others. Golden fixtures and
`kind_coverage.literals`, however, currently advertise only 9 languages:

`typescript`, `javascript`, `vue`, `csharp`, `ruby`, `lua`, `r`, `bash`,
`powershell`.

Impact: this is the cheapest broad coverage win. The code/test evidence
already exists for much of the matrix, but the product contract does not expose
it.

### 3. Complexity metrics remain uneven

Current fixture-proven languages:

`rust`, `c`, `cpp`, `go`, `typescript`, `javascript`, `python`, `java`,
`csharp`, `swift`, `kotlin`, `dart`.

Likely code-language targets still missing:

`zig`, `tsx`, `jsx`, `vue`, `vbnet`, `php`, `ruby`, `scala`, `elixir`,
`lua`, `qml`, `r`, `bash`, `powershell`, `gdscript`, and `razor`.
SQL should be design-gated separately because procedural SQL complexity is not
the same metric as cyclomatic complexity in general-purpose code.

Several format or markup languages should probably become explicit
not-applicable rows: `json`, `toml`, `yaml`, `markdown`, `regex`, and likely
`css`.

### 4. Identifier richness is weak in specific languages

Low current identifier variety:

- `jsx`: only `call`.
- `vue`: only `call`.
- `bash`: only `call`.
- `sql`: only `member_access`.
- `yaml`: only `variable_ref`.

Some of these are legitimate format constraints, but Bash, JSX, Vue, and SQL
should be treated as real extraction-quality follow-ups before marking them
complete.

### 5. Doc-comment coverage improved but policy is still inconsistent

Golden evidence exists for 23 languages. Missing or unaudited languages include
`c`, `cpp`, `zig`, `tsx`, `vbnet`, `scala`, `elixir`, `lua`, `qml`, `r`,
`gdscript`, plus likely not-applicable rows for `regex` and `yaml`.

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

### 7. Structural facts need a product bar

Only 12 languages emit structural facts. That is acceptable only if the product
bar is "high-value facts where the language has a stable, useful construct,"
not "every language emits at least one." The current matrix does not document
that distinction.

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

## Recommended next plan

Build the next pass around a matrix policy plus language-family slices:

1. Add a no-silent-empty-domain policy to `capabilities.json` and
   `capability_matrix.rs`.
2. Close the literal golden/capability mismatch first.
3. Normalize doc-comment policy and fill missing fixture evidence.
4. Audit annotation helpers and turn hidden support into claims or explicit
   gaps.
5. Expand complexity metrics for code languages, while marking data/markup
   languages not applicable.
6. Improve identifiers and type-argument usage where existing tests already
   indicate deeper language support.
7. Add targeted structural facts only where they have clear downstream value.
8. Fix the known Dart, C#, and SQL quality defects as separate, test-first
   tasks.
