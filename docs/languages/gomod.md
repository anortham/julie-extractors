# Go module manifest support

`gomod` handles Go module manifests. It has no extension mapping: a file is
selected when its basename is exactly `go.mod`, compared case-insensitively
like `qmldir`, so `GO.MOD` on a case-insensitive Windows volume selects it too.
`go.sum` selects the separate [`gosum`](gosum.md) language. `go.work` is not
selected.

The parser is `tree-sitter-gomod`, pinned to the owned fork
`anortham/tree-sitter-go-mod` at `655fba88f326a7c8588d118c716428d038296189`
(see the [grammar dependency policy](../architecture/grammar-dependency-policy.md)).
The fork adds the `godebug` directive, accepts absolute and one-character file
paths, and accepts a last line with no final newline.

## Rows

One directive line is one row source: a single-line directive, or one line of
a `( ... )` block. A line's span runs from the keyword (single-line form) or
the first value (block form) to its last value, so a trailing comment stays
outside it.

| Directive | Symbol | Body span | Other rows |
| --- | --- | --- | --- |
| `module` | `module`, public, named by the module path | module path | `gomod.module.v1` |
| `go` | `property` named `go` | version | `gomod.go.v1` |
| `toolchain` | `property` named `toolchain` | toolchain name | `gomod.toolchain.v1` |
| `godebug` | `property` named by the key | value | `gomod.godebug.v1` |
| `require` | `import` named by the module path | version | `manifest.dependency.v1`, `Imports` edge |
| `tool` | `import` named by the package path | package path | `gomod.tool.v1`, `Imports` edge |
| `replace` | none | none | `gomod.replace.v1`; pending row for a file path |
| `exclude` | none | none | `gomod.exclude.v1` |
| `retract` | none | none | `gomod.retract.v1` |
| `ignore` | none | none | `gomod.ignore.v1` |

- A symbol's signature is `keyword values`, with `// indirect` appended for an
  indirect requirement. A `require` symbol also carries `version` and
  `indirect` metadata, and the `module` symbol carries `deprecated`.
- The contiguous `//` lines directly above a line are its doc comment and
  `doc_comment` source regions. A blank line detaches them. Other comments,
  such as `// indirect`, are `comment` regions.
- Each `require` and `tool` symbol is the target of an `Imports` edge from the
  module symbol, with `dependencyKind` (`require` or `tool`) and
  `dependencyName` metadata, plus `indirect` for `require`. A file without a
  `module` line publishes no edges.
- A `replace` whose target is a file path is a structured pending `Imports`
  row from the module symbol to `<path>/go.mod`, with `terminal_name` `go.mod`
  and `import_context` `<path>/go.mod`. Backslashes become `/`. This is the
  Maven `module` precedent: code-kb resolves the file at query time.
- A quoted value (`"..."` or a raw string) is unquoted for every name and
  fact. It is also a `string_literal` source region and an `other` literal
  whose carrier is `<directive>.<role>`, such as `require.module_path`.

## Structural facts

Every fact has `query_family` `dependencies`.

- `manifest.dependency.v1`: ecosystem `go`, `name` (module path), group
  `require`, `version`, and `indirect`. `indirect` follows
  `modfile.isIndirect`: the suffix comment is `// indirect` or starts with
  `// indirect;`.
- `gomod.module.v1`: `module_path`, and `deprecated` from the `Deprecated:`
  paragraph of the leading or suffix comment (`modfile.parseDeprecation`).
- `gomod.go.v1`: `version`. `gomod.toolchain.v1`: `toolchain`.
- `gomod.replace.v1`: `module_path`, optional `version`, `replacement`,
  optional `replacement_version`, and `local` (true for a file path).
- `gomod.exclude.v1`: `module_path` and `version`.
- `gomod.retract.v1`: `low`, `high` (both the version for a single version),
  `range`, and `rationale`. The rationale is the line's leading and suffix
  comments, else those of its block, as `modfile` reads it. It keeps at most
  500 bytes, cut at a character boundary; a cut rationale also sets
  `rationale_truncated`. Every line of a block shares the block comment, so the
  cut keeps one long comment from being copied whole into every fact.
- `gomod.tool.v1`: `package_path`. `gomod.ignore.v1`: `path`.
- `gomod.godebug.v1`: `key` and `value` of one `key=value` setting.

Identifiers and types are typed exceptions: a manifest has no calls,
variables, member access, or type system.

## Known gaps

The `gomod` row has no open gaps.

## Continuous testing

```bash
cargo xtask test language gomod
```

The command runs `tests::gomod::` and the golden check with
`JULIE_GOLDEN_LANGUAGE=gomod`. The fixtures are `fixtures/extraction/gomod/basic`
(every directive in single-line and block form, an absolute-path replacement, a
one-character ignore path, and a last line with no final newline) and
`fixtures/extraction/gomod/negative` (no comments, no pending rows for a
module replacement, and no edges from `replace` or `exclude`).

## Real-file evidence

On 2026-09-23 the release CLI scanned a temporary tree that held copies of the
128 `go.mod` files under `~/go/pkg/mod` and `~/source`: 128 files, status `ok`,
0 failed, 0 parse diagnostics. The artifact held 754 symbols (128 `module`,
123 `property`, 503 `import`), 503 `Imports` edges, 762 structural facts (503
`manifest.dependency.v1` with 253 indirect, 128 `gomod.module.v1`, 121
`gomod.go.v1`, 8 `gomod.retract.v1`, 2 `gomod.toolchain.v1`), 274 source
regions, and 2 literals from the quoted paths in `gopkg.in/yaml.v3`. A rescan
with the forked grammar gave the same counts. None of those files has a
`godebug` line.

A scan of `~/source/cobra` selected its `go.mod` as `gomod` with 6 symbols, 4
`Imports` edges, 6 structural facts, and 0 parse diagnostics, and its `go.sum`
as `gosum`.
