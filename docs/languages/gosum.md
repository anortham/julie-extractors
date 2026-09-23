# Go checksum support

`gosum` handles Go checksum files. It has no extension mapping: a file is
selected when its basename is exactly `go.sum`, compared case-insensitively
like `go.mod`, so `GO.SUM` on a case-insensitive Windows volume selects it too.
`go.work.sum` is not selected.

The parser is `tree-sitter-gosum`, pinned to the owned fork
`anortham/tree-sitter-go-sum` at `b1727178d527bf4171dc4772afca3d220def49cd`
(see the [grammar dependency policy](../architecture/grammar-dependency-policy.md)).
The fork accepts an empty file and any semver pre-release, such as
`v0.1.1-deprecated`. The upstream grammar rejected both, so 11 of 70 local
`go.sum` files failed to parse.

## Rows

A `go.sum` line is `<module> <version>[/go.mod] h1:<hash>`: the hash the go
command verified for a module version, or for its `go.mod` file alone. The
file declares nothing and can keep lines for modules the build no longer uses,
so `gosum` publishes no symbols, edges, identifiers, or types. Each false
capability is a typed exception on the capability row. The dependency symbols
and `Imports` edges come from the [`gomod`](gomod.md) row; a consumer joins
the two by module path and version.

Every line that parses cleanly is one `gosum.checksum.v1` structural fact with
`query_family` `dependencies`. Its span is the line without the line break.

| Key | Value |
| --- | --- |
| `module_path` | The module path. |
| `version` | The version, without the `/go.mod` suffix. |
| `go_mod` | True for a `<version>/go.mod` line, which hashes only the `go.mod` file. |
| `hash_algorithm` | The prefix before `:`, such as `h1`. |
| `hash` | The base64 hash after the prefix. |
| `incompatible` | True when the version ends in `+incompatible`. |
| `pseudo_version` | True when `module.IsPseudoVersion` accepts the version. |
| `timestamp` | For a pseudo-version, the UTC commit time `yyyymmddhhmmss`. |
| `revision` | For a pseudo-version, the commit hash prefix. |

- The file has no comments or string literals, so it has no source regions,
  literals, or `code.marker.v1` facts.
- A malformed line reports a parse diagnostic and publishes no fact. Tree-sitter
  can fold the next line into the same broken record, so that line can be lost
  too. The go command rejects such a file.
- An artifact built with `--level symbols` holds no structural facts, so a
  `go.sum` file has no rows at that level.

## Continuous testing

```bash
cargo xtask test language gosum
```

The command runs `tests::gosum::` and the golden check with
`JULIE_GOLDEN_LANGUAGE=gosum`. The fixtures are
`fixtures/extraction/gosum/basic` (real lines with `/go.mod` hashes, all three
pseudo-version forms, `+incompatible`, and pre-releases) and
`fixtures/extraction/gosum/empty` (an empty file with no rows and no
diagnostics).

## Real-file evidence

On 2026-09-23 the release CLI scanned a temporary tree that held copies of the
70 `go.sum` files under `~/go/pkg/mod` and `~/source`, 7 of them empty: 70
files, status `ok`, 0 failed, 0 parse diagnostics. The artifact held 2774
`gosum.checksum.v1` facts, one per non-blank line: 2136 `go_mod` hashes, 978
pseudo-versions, 9 `+incompatible` versions, and 308 distinct modules.

A scan of `~/source/cobra` selected its `go.sum` as `gosum` with 12 facts and
0 parse diagnostics.
