# Razor parser hardening evidence — 2026-07-13

## Certification identity

- julie-extractors commit tested: `37d6941909ba4d31f5979533002019e5bf19212c`
- tree-sitter-razor commit: `fba8571f06c06aa5acca01e3d762f5a5e78dc50f`
- Parser ABI: `15`
- Parser certification document commit: `9fdcfd755d5537e8285166c25c34d1617bdf0826`
- Terraform commit: `821e6b1a268cb392b1abb5080243a299db2a9bc9`
- Documentation-corpus replay commit: `37d6941909ba4d31f5979533002019e5bf19212c`
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
documentation snippets produced 14 diagnostics in total: five are the preview
`Virtualize` and `QuickGrid` examples containing literal documentation ellipses
(`...`), and nine come from stable placeholder or pseudocode fragments. The
valid mixed component-attribute and complete Razor-template/`RenderFragment`
examples that previously exposed parser limitations now produce zero
diagnostics. No valid documentation example has an unexpected diagnostic. The
complete Terraform corpus below remains the zero-diagnostic hard gate.

### Rebuild and replay the documentation profile

The Learn URLs above label the stable/preview status. Replay uses immutable raw
files at the recorded `AspNetCore.Docs` commit. These commands reconstruct the
profile and its manifest after `/tmp` has been deleted:

```bash
set -euo pipefail
commit=71011a30140248dffcc6a757d435670365f523d2
sources=/tmp/julie-razor-doc-sources-71011a3
profile=/tmp/julie-razor-doc-corpus-71011a3
base="https://raw.githubusercontent.com/dotnet/AspNetCore.Docs/$commit/aspnetcore"

rm -rf "$sources" "$profile"
mkdir -p "$sources" "$profile"
curl -fsSLo "$sources/razor-syntax.md" "$base/mvc/views/razor.md"
curl -fsSLo "$sources/razor-components.md" "$base/blazor/components/index.md"
curl -fsSLo "$sources/blazor-preview.md" \
  "$base/release-notes/aspnetcore-11/includes/blazor.md"

cat > "$sources/SHA256SUMS" <<'SUMS'
c527358cbdb45ea6a00bb689ff73a50ac8adb68a8a4a1a1edc283e51e8f562b9  blazor-preview.md
f7f2511d955976974d0774c0fac1befb6f7ffbaf76709170450fefcfaf3f5621  razor-components.md
4ff7059340a0cc3dc5ad53848362e7959bc7a2d111d448f96e38807fe0d67870  razor-syntax.md
SUMS
(cd "$sources" && shasum -a 256 -c SHA256SUMS)

python3 - <<'PY'
from pathlib import Path
import json

commit = "71011a30140248dffcc6a757d435670365f523d2"
sources = Path("/tmp/julie-razor-doc-sources-71011a3")
out = Path("/tmp/julie-razor-doc-corpus-71011a3")

specs = [
    ("stable-syntax", sources / "razor-syntax.md", "stable",
     "https://learn.microsoft.com/en-us/aspnet/core/mvc/views/razor?view=aspnetcore-10.0", 48),
    ("stable-components", sources / "razor-components.md", "stable",
     "https://learn.microsoft.com/en-us/aspnet/core/blazor/components/?view=aspnetcore-10.0", 26),
    ("preview-blazor", sources / "blazor-preview.md", "preview",
     f"https://github.com/dotnet/AspNetCore.Docs/blob/{commit}/aspnetcore/release-notes/aspnetcore-11/includes/blazor.md", 9),
]

def fences(path):
    heading = ""
    opened = None
    body = []
    for line_number, line in enumerate(path.read_text().splitlines(), 1):
        stripped = line.strip()
        if opened is None and stripped.startswith("#"):
            heading = stripped.lstrip("#").strip()
        if opened is None and stripped.startswith("```"):
            language = stripped[3:].strip().split(" ", 1)[0]
            opened = (language, line_number, heading)
            body = []
        elif opened is not None and stripped == "```":
            yield (*opened, body)
            opened = None
            body = []
        elif opened is not None:
            body.append(line)
    assert opened is None, f"unterminated fence in {path}"

manifest = []
for prefix, path, channel, url, expected in specs:
    selected = [block for block in fences(path)
                if block[0] in {"razor", "cshtml"}]
    assert len(selected) == expected, (path, len(selected), expected)
    for index, (language, source_line, heading, body) in enumerate(selected, 1):
        name = f"{prefix}-{index:02d}.razor"
        (out / name).write_text("\n".join(body) + "\n")
        manifest.append({
            "file": name, "channel": channel, "url": url,
            "source_commit": commit, "source_line": source_line,
            "heading": heading, "language": language,
        })

preview = sources / "blazor-preview.md"
assert "<BasePath />" in preview.read_text()
(out / "preview-base-path-markup.razor").write_text("<BasePath />\n")
manifest.append({
    "file": "preview-base-path-markup.razor", "channel": "preview",
    "url": specs[2][3], "source_commit": commit, "source_line": 70,
    "heading": "New BasePath component", "language": "markup",
})
mathml = [body for language, _, heading, body in fences(preview)
          if language == "html" and heading == "MathML namespace support"]
assert len(mathml) == 1
(out / "preview-mathml-markup.razor").write_text("\n".join(mathml[0]) + "\n")
manifest.append({
    "file": "preview-mathml-markup.razor", "channel": "preview",
    "url": specs[2][3], "source_commit": commit, "source_line": 461,
    "heading": "MathML namespace support", "language": "html",
})

assert len(manifest) == 85
(out / "manifest.json").write_text(json.dumps({
    "retrieved": "2026-07-13", "source_commit": commit, "inputs": manifest,
}, indent=2) + "\n")
print(json.dumps({
    "razor_inputs": len(manifest),
    "stable": sum(item["channel"] == "stable" for item in manifest),
    "preview": sum(item["channel"] == "preview" for item in manifest),
    "preview_razor_fences": sum(
        item["file"].startswith("preview-blazor-") for item in manifest),
}))
PY
```

Expected manifest summary:
`{"razor_inputs": 85, "stable": 74, "preview": 11, "preview_razor_fences": 9}`.

Build and run the product CLI twice:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
profile=/tmp/julie-razor-doc-corpus-71011a3
db=/tmp/julie-razor-doc-corpus-71011a3.sqlite
scan=/tmp/julie-razor-doc-corpus-71011a3-scan.json
rescan=/tmp/julie-razor-doc-corpus-71011a3-rescan.json
rm -f "$db" "$scan" "$rescan"
target/release/julie-extract scan --root "$profile" --db "$db" --json > "$scan"
target/release/julie-extract scan --root "$profile" --db "$db" --json > "$rescan"
jq -e '.status == "ok" and .counts.files_failed == 0 and
  .profile.languages.razor.files == 85 and
  .profile.languages.razor.failed_files == 0' "$scan" >/dev/null
jq -e '.status == "no_change" and .counts.files_changed == 0 and
  .profile.languages.razor.unchanged_files == 85' "$rescan" >/dev/null
```

Validate the named clean cases and classify every diagnostic:

```bash
db=/tmp/julie-razor-doc-corpus-71011a3.sqlite
sqlite3 -header -column "$db" "
SELECT f.path, COUNT(d.file_id) AS diagnostics
FROM files f
LEFT JOIN parse_diagnostics d
  ON d.file_id=f.file_id AND d.language='razor'
WHERE f.path IN (
  'preview-blazor-01.razor', 'preview-base-path-markup.razor',
  'preview-blazor-02.razor', 'preview-blazor-08.razor',
  'preview-mathml-markup.razor', 'preview-blazor-09.razor',
  'stable-components-12.razor', 'stable-components-19.razor')
GROUP BY f.path ORDER BY f.path;
SELECT CASE WHEN f.path LIKE 'preview-%' THEN 'preview' ELSE 'stable' END channel,
       COUNT(*) diagnostics
FROM parse_diagnostics d JOIN files f ON f.file_id=d.file_id
WHERE d.language='razor' GROUP BY channel ORDER BY channel;
SELECT f.path, COUNT(*) diagnostics
FROM parse_diagnostics d JOIN files f ON f.file_id=d.file_id
WHERE d.language='razor' GROUP BY f.path ORDER BY f.path;
PRAGMA integrity_check;
SELECT COUNT(*) AS razor_files FROM files WHERE language='razor';
SELECT COUNT(*) AS razor_diagnostics FROM parse_diagnostics WHERE language='razor';"
```

Expected results are zero diagnostics for all eight named or formerly failing
valid files, five preview and nine stable diagnostics, SQLite integrity `ok`,
85 Razor files, and 14 total Razor diagnostics. Every remaining row is an
explicit placeholder or pseudocode input:

| File | Diagnostics | Classification |
|---|---:|---|
| `preview-blazor-03.razor` | 1 | Literal `...` placeholder |
| `preview-blazor-04.razor` | 2 | Literal `...` placeholders |
| `preview-blazor-05.razor` | 2 | Literal `...` placeholders |
| `stable-components-10.razor` | 1 | `await ...` placeholder |
| `stable-components-11.razor` | 1 | `await ...` placeholder |
| `stable-components-18.razor` | 1 | Razor-template pseudocode placeholder |
| `stable-syntax-21.razor` | 1 | Collection `...` placeholder |
| `stable-syntax-34.razor` | 2 | Function-body `...` placeholder |
| `stable-syntax-40.razor` | 2 | `<...>` pseudocode placeholder |
| `stable-syntax-44.razor` | 1 | Razor-template pseudocode placeholder |

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
- JSONL: schema 3, 103,079 valid records, 94,211,934 bytes.
- Reports: scan `ok`, rescan `no_change`, info `ok`, export `ok`; no errors or
  warnings.
- Parser inventory: `tree-sitter-razor` 0.1.1 from exact git source
  `fba8571f06c06aa5acca01e3d762f5a5e78dc50f`.
- Report-only timing: scan 1,747 ms, rescan 75 ms, info 16 ms, export 545 ms,
  4,347.68 rows/s.

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
