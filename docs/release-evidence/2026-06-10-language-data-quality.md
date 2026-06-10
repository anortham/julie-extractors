# Language Data Quality Dogfood Evidence

Date: 2026-06-10

Branch under test: `feature/extraction-data-quality`

Commit under test: `78bf354c49ad2884e1ca0d785ddec815ac0a7aec`

## Scope

This evidence covers Phase 6 of
`docs/plans/2026-06-10-language-data-quality.md`: downstream dogfood and
comparative quality.

The scan set includes the three active dependent projects plus representative
local repositories across major language families:

- Dependent projects: `miller`, `julie`, `eros`
- Product self-scan: `julie-extractors`
- Representative corpora: `blazor-samples`, `rasd-vue-library`, `express`,
  `flask`, `zod`, `Newtonsoft.Json`, `Alamofire`, `jq`, `nlohmann-json`,
  `gson`, `phoenix`, `sinatra`

Full SQLite artifacts and `scan.json` reports are under:

```text
target/data-quality-dogfood/2026-06-10/
target/data-quality-dogfood/2026-06-10-baseline-v2.2.1/
```

## Commands

Current branch binary:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
target/release/julie-extract --version
```

Result:

```text
julie-extract 2.2.1
```

Baseline binary:

```bash
gh release download v2.2.1 \
  --repo anortham/julie-extractors \
  --pattern 'julie-extract-v2.2.1-aarch64-apple-darwin.tar.gz' \
  --dir target/release-verification/v2.2.1
tar -xzf \
  target/release-verification/v2.2.1/julie-extract-v2.2.1-aarch64-apple-darwin.tar.gz \
  -C target/release-verification/v2.2.1/extract-aarch64
target/release-verification/v2.2.1/extract-aarch64/dist/aarch64-apple-darwin/julie-extract --version
```

Result:

```text
julie-extract 2.2.1
```

Each repository was scanned with the same shape:

```bash
target/release/julie-extract scan \
  --root <repo-root> \
  --db target/data-quality-dogfood/2026-06-10/<repo>/artifact.sqlite \
  --force \
  --strict-schema \
  --json
```

The baseline scan used the downloaded v2.2.1 binary and wrote to
`target/data-quality-dogfood/2026-06-10-baseline-v2.2.1/<repo>/`.

The strict scorecard also passed:

```bash
node scripts/language-data-quality-report.mjs --strict
```

Summary:

```text
silent_cells: 0
quality_bar_debts: 0
Silent Cells: none
Quality-Bar Debt: none
```

## Hard Gate Result

All 16 current-branch scans completed successfully with `--strict-schema`.

- Repositories scanned: `16`
- Files recorded: `12893`
- Failed files: `0`
- Parse diagnostics: `18517`

The baseline v2.2.1 scan also had `0` failed files and the same parse
diagnostic count. The quality branch did not increase parser recovery noise in
this dogfood set.

## Domain Totals

| Domain | v2.2.1 | Branch | Delta |
| --- | ---: | ---: | ---: |
| `files` | 12893 | 12893 | +0 |
| `symbols` | 616955 | 616955 | +0 |
| `identifiers` | 1156556 | 1156556 | +0 |
| `relationships` | 43050 | 43119 | +69 |
| `type_facts` | 175345 | 175474 | +129 |
| `literals` | 1437 | 1437 | +0 |
| `source_regions` | 1139901 | 1139901 | +0 |
| `structural_facts` | 8048 | 459430 | +451382 |
| `complexity_metrics` | 44949 | 78636 | +33687 |
| `parse_diagnostics` | 18517 | 18517 | +0 |

`literals` and `source_regions` row counts are stable in this corpus. The
branch improvements for those domains are quality improvements to carriers and
embedded-region metadata, proven by fixture tests rather than new downstream
row volume.

## Per-Repository Deltas

| Repo | Files | Failed | Parse diagnostics | Structural facts delta | Source regions delta | Literals delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `julie-extractors` | 1180 | 0 | 9 | +68891 | +0 | +0 |
| `miller` | 552 | 0 | 22 | +2933 | +0 | +0 |
| `julie` | 1561 | 0 | 153 | +18875 | +0 | +0 |
| `eros` | 619 | 0 | 11 | +75085 | +0 | +0 |
| `blazor-samples` | 4839 | 0 | 1873 | +115741 | +0 | +0 |
| `rasd-vue-library` | 213 | 0 | 8 | +1427 | +0 | +0 |
| `express` | 165 | 0 | 24 | +5376 | +0 | +0 |
| `flask` | 130 | 0 | 11 | +5427 | +0 | +0 |
| `zod` | 451 | 0 | 30 | +20155 | +0 | +0 |
| `newtonsoft-json` | 981 | 0 | 285 | +2131 | +0 | +0 |
| `alamofire` | 478 | 0 | 118 | +84411 | +0 | +0 |
| `jq` | 84 | 0 | 500 | +10470 | +0 | +0 |
| `nlohmann-json` | 776 | 0 | 8414 | +10869 | +0 | +0 |
| `gson` | 284 | 0 | 0 | +4544 | +0 | +0 |
| `phoenix` | 411 | 0 | 7059 | +19455 | +0 | +0 |
| `sinatra` | 169 | 0 | 0 | +5592 | +0 | +0 |

## Structural Fact Deltas By Language

| Language | v2.2.1 | Branch | Delta |
| --- | ---: | ---: | ---: |
| `bash` | 0 | 122 | +122 |
| `css` | 0 | 86373 | +86373 |
| `dart` | 0 | 30 | +30 |
| `elixir` | 0 | 4885 | +4885 |
| `gdscript` | 0 | 22 | +22 |
| `html` | 67 | 67165 | +67098 |
| `java` | 0 | 3646 | +3646 |
| `json` | 0 | 207096 | +207096 |
| `kotlin` | 0 | 10 | +10 |
| `lua` | 0 | 23 | +23 |
| `markdown` | 0 | 25764 | +25764 |
| `php` | 0 | 13 | +13 |
| `powershell` | 0 | 144 | +144 |
| `qml` | 0 | 303 | +303 |
| `r` | 0 | 4 | +4 |
| `razor` | 2469 | 10731 | +8262 |
| `regex` | 0 | 776 | +776 |
| `ruby` | 0 | 4340 | +4340 |
| `scala` | 0 | 12 | +12 |
| `sql` | 0 | 1759 | +1759 |
| `swift` | 0 | 1129 | +1129 |
| `toml` | 0 | 4074 | +4074 |
| `vbnet` | 0 | 7 | +7 |
| `vue` | 0 | 793 | +793 |
| `yaml` | 0 | 34645 | +34645 |
| `zig` | 0 | 52 | +52 |

## Parse Diagnostics

Parse diagnostics are unchanged from baseline in aggregate. They are still
worth tracking because several large real-world corpora exercise parser
recovery heavily.

Top current-branch diagnostic totals by language:

| Language | Diagnostics | Files | Diagnostics / file |
| --- | ---: | ---: | ---: |
| `cpp` | 8413 | 486 | 17.31 |
| `elixir` | 6920 | 295 | 23.46 |
| `razor` | 1484 | 3084 | 0.48 |
| `c` | 500 | 53 | 9.43 |
| `css` | 494 | 314 | 1.57 |
| `csharp` | 238 | 2266 | 0.11 |
| `html` | 158 | 472 | 0.33 |
| `swift` | 108 | 101 | 1.07 |
| `javascript` | 68 | 307 | 0.22 |
| `powershell` | 52 | 14 | 3.71 |
| `typescript` | 22 | 503 | 0.04 |
| `markdown` | 19 | 916 | 0.02 |

## Interpretation

This branch materially raises downstream extraction depth without lowering the
support bar:

- It preserves core symbol, identifier, relationship, literal, source-region,
  and parse-diagnostic totals across the scanned corpus.
- It adds broad structural-fact coverage across data, markup, style, regex,
  scripting, JVM, .NET, Swift, Ruby, Elixir, SQL, and shell-family languages.
- It increases complexity rows where previously supported languages had only
  fixture-level proof but not broad dogfood volume.
- It does not introduce failed-file rows in dependent projects or the
  representative corpora.

Remaining closeout work is documentation and final branch validation, not
another language-by-language implementation slice.
