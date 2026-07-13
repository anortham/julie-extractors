# Razor parser hardening evidence — 2026-07-13

## Certification identity

- julie-extractors commit tested: `8b9a860b379a60fab1ff2c034cc6f01a05998395`
- tree-sitter-razor commit: `e38a509720eb54652d7079380acaa62064a2c66a`
- Parser ABI: `15`
- Parser certification document commit: `faa4da5`
- Terraform commit: `821e6b1a268cb392b1abb5080243a299db2a9bc9`
- This is implementation evidence only; no release, tag, publication, or version
  decision is claimed.

## Documentation corpus

Inputs were retrieved on 2026-07-13 and assembled as the explicitly named local
profile `/tmp/julie-razor-doc-corpus-71011a3`:

- Stable: [.NET 10 Razor syntax reference](https://learn.microsoft.com/en-us/aspnet/core/mvc/views/razor?view=aspnetcore-10.0),
  48 Razor or CSHTML fences.
- Stable: [.NET 10 Razor components](https://learn.microsoft.com/en-us/aspnet/core/blazor/components/?view=aspnetcore-10.0),
  26 Razor or CSHTML fences.
- Preview: [`aspnetcore-11/includes/blazor.md`](https://github.com/dotnet/AspNetCore.Docs/blob/71011a30140248dffcc6a757d435670365f523d2/aspnetcore/release-notes/aspnetcore-11/includes/blazor.md)
  at `dotnet/AspNetCore.Docs` commit
  `71011a30140248dffcc6a757d435670365f523d2`, all 9 Razor fences plus the
  `BasePath` and MathML markup examples that are not Razor-fenced upstream.

The profile contains 85 Razor inputs. All were processed with zero failed files,
and the immediate rescan reported `no_change`. Named inputs for `DisplayName`,
`BasePath`, `NavLink RelativeToCurrentUri`, `EnvironmentBoundary`, MathML, and
asynchronous form validation each produced zero diagnostics. The exact isolated
documentation snippets produced 29 diagnostics in total: five are the preview
`Virtualize` and `QuickGrid` examples containing literal documentation ellipses
(`...`), and the rest are incomplete stable-page fragments. These are classified
profile inputs, not unexpected diagnostics in a complete source corpus.

## Verification ledger

| Invariant | Command | Result |
|---|---|---|
| Certification contracts remain green | `cargo xtask test certification` | PASS: 39 capability, 1 pending-shape, 2 parser-upgrade tests |
| Specialist real-world fixture remains green outside the default tier | `cargo xtask test real-world-smoke` | PASS: 1 test |
| Product CLI builds against the certified parser | `cargo build --release -p julie-extract-cli --bin julie-extract` | PASS |
| Live Terraform corpus and immediate rescan satisfy the hard gates | `cargo xtask dogfood repo --root /Users/murphy/source/Terraform --out-dir target/dogfood/terraform-razor-hardening --binary target/release/julie-extract` | PASS |
| Capability evidence has no silent cells or quality debt | `node scripts/language-data-quality-report.mjs --strict` | PASS: `silent_cells=0`, `quality_bar_debts=0` |

No corpus, certification, or real-world command was added to the default test
tier; this task changes documentation only.

## Terraform evidence

`git -C /Users/murphy/source/Terraform ls-files '*.razor' | wc -l` returned 28.
The release-binary dogfood scan then produced:

- Razor: 28/28 processed, 28 changed, zero failed, zero parse diagnostics,
  162,750 source bytes.
- Full scan: 418 discovered, 388 indexed, 30 unsupported, zero failed.
- Immediate rescan: status `no_change`; 388 indexed files unchanged, including
  all 28 Razor files; zero changed, deleted, or failed.
- SQLite: integrity `ok`, schema 4, extract contract 3, 88,182,784 bytes.
- JSONL: schema 3, 103,079 valid records, 94,211,552 bytes.
- Reports: scan `ok`, rescan `no_change`, info `ok`, export `ok`; no errors or
  warnings.
- Parser inventory: `tree-sitter-razor` 0.1.1 from exact git source
  `e38a509720eb54652d7079380acaa62064a2c66a`.
- Report-only timing: scan 2,925 ms, rescan 81 ms, info 19 ms, export 556 ms,
  2,596.74 rows/s.

Artifacts are under `target/dogfood/terraform-razor-hardening/`:

- `artifact.sqlite`
- `artifact.jsonl`
- `scan-report.json`
- `rescan-report.json`
- `info-report.json`
- `export-report.json`
- `metrics.json`

The historical `69/69` value was a Razor test count. Prior release-binary
evidence measured 28 Terraform Razor files, and the current live measurement is
also 28. Therefore, the apparent 69-to-28 drift is a comparison of different
units, not a corpus regression.

## Reproducible readback

```bash
git -C /Users/murphy/source/Terraform ls-files '*.razor' | wc -l
sqlite3 target/dogfood/terraform-razor-hardening/artifact.sqlite \
  "PRAGMA integrity_check; SELECT COUNT(*) FROM files WHERE language='razor'; SELECT COUNT(*) FROM parse_diagnostics WHERE language='razor';"
jq -c '{status,files_failed:.counts.files_failed,razor:.profile.languages.razor}' \
  target/dogfood/terraform-razor-hardening/scan-report.json \
  target/dogfood/terraform-razor-hardening/rescan-report.json
jq -c . target/dogfood/terraform-razor-hardening/artifact.jsonl >/dev/null
wc -l target/dogfood/terraform-razor-hardening/artifact.jsonl
```
