# QML support

Julie registers two QML-family languages:

- `qml` handles `.qml` and `.qmltypes` files.
- `qmldir` handles files whose exact basename is `qmldir`. It intentionally
  has no extension mapping; `.qmldir` is not a supported spelling.

## Continuous testing

Run the language targets when changing QML extraction:

```bash
cargo xtask test language qml
cargo xtask test language qmldir
```

Each command runs the matching unit-test module and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE` set to the canonical capability-matrix language.
The normal golden target remains unfiltered:

```bash
cargo xtask test golden
```

`qmltypes` is an input extension, not a separate test target. Use `qml` for
both QML source and generated QML type metadata.

## Grammar freshness

The live maintenance report was run with:

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The QML-specific findings were:

- `tree-sitter-qmldir` is current: pinned and locked at
  `c57e00865a1a6f1cca83340d6dad91f13df55479`, matching the remote head.
- `tree-sitter-qmljs` is marked drift: pinned and locked at
  `606a66b96a13ef30ed5c7ec7e5adc20a9a40157a`; the report observed remote
  head `de96ed62abded51fcdfcbeaaa120e0dd0d20c697`.
- The shared `tree-sitter` runtime is also marked drift at locked `0.26.11`
  versus latest stable `0.26.13`; this is a repository-wide freshness finding,
  not an unrecorded QML dependency change.

## Real-world evidence

The evidence corpus was KDE `plasma-framework` at commit
`0806864a1e7c200ee8872074a4c16be7e1ce3358`. It was cloned shallowly into a
temporary directory and no project build scripts, hooks, or third-party
binaries were executed.

The checkout is multi-licensed. Its SPDX headers and `LICENSES/` directory
include LGPL-2.0-or-later, LGPL-2.1/LGPL-3.0 combinations with
`LicenseRef-KDE-Accepted-LGPL`, GPL-2.0-or-later, and Qt commercial exception
expressions. Treat source redistribution as subject to the repository's
per-file license metadata.

Reproducible checkout and scan commands:

```bash
CORPUS="$(mktemp -d)"
git clone --depth 1 https://github.com/KDE/plasma-framework "$CORPUS"
git -C "$CORPUS" fetch --depth 1 origin \
  0806864a1e7c200ee8872074a4c16be7e1ce3358
git -C "$CORPUS" checkout --detach \
  0806864a1e7c200ee8872074a4c16be7e1ce3358

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

The filesystem audit found 179 `.qml` files, one `.qmltypes` file, and five
exact-basename `qmldir` files. The scan report was `status=ok` with
`files_scanned=751`, `files_changed=384`, `files_unsupported=367`,
`files_failed=0`, and empty `warnings` and `errors`. The report's per-file
section was truncated by the CLI contract, so language-specific counts below
come from the SQLite artifact.

| Artifact evidence | `qml` | `qmldir` |
| --- | ---: | ---: |
| Indexed files | 180 (179 `.qml` + 1 `.qmltypes`) | 5 |
| Symbols | 7,195 | 53 |
| Structural facts | 9,884 | 53 |
| Resolved relationships | 1,112 | 0 |
| Pending relationships | 2,360 | 0 |
| Parse diagnostics | 121 | 10 |

The diagnostics are parser diagnostics recorded in the artifact; they did not
fail the scan. The 121 QML diagnostics break down into 115 CMake-template
`@QQC2_VERSION@` placeholders, 4 `%{APPNAMELC}` project-template
placeholders, and 2 intentional empty test fixtures. The 10 qmldir
diagnostics are the same `%{APPNAMELC}` placeholders across two
project-template manifests. Re-running extraction after substituting those
template values yielded zero diagnostics; no valid-QML grammar limitation or
extractor bug was found.

Representative rows prove both registrations:

- `src/declarativeimports/core/plugins.qmltypes` was indexed as `qml` with
  750 symbols and 2,375 structural facts, including 771
  `qml.typeinfo_declaration.v1` facts.
- `src/declarativeimports/plasmacomponents3/qmldir` was indexed as `qmldir`
  with 41 symbols and 41 structural facts, including one module fact and 40
  object-type facts.

The temporary checkout and SQLite artifact were removed after recording this
evidence.
