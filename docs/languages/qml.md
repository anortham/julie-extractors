# QML support

Julie registers two QML-family languages:

- `qml` handles `.qml` and `.qmltypes` files.
- `qmldir` handles files whose exact basename is `qmldir`. It intentionally
  has no extension mapping; `.qmldir` is not a supported spelling.

## The object model in symbols

A `.qml` file's root object is a `class` symbol: the file names the component.
A Qt Quick UI Form (`Screen01.ui.qml`) names the component `Screen01`, because
QML code uses it as `Screen01 {}`. Its base type is not a symbol; it is an
`extends` relationship (see *The reference contract* below).

Inline components (`component Badge: Rectangle { ... }`) are `class` symbols
too, with the signature `component Badge: Rectangle`.

Every other nested object is a `field` symbol:

- The name is the object's `id` when it declares one, else its type name.
- The signature is `id: Type`, or the bare type when there is no `id`.
- The metadata carries `object_type` (the declared type) and
  `binding_kind: "object"`.
- A value source (`Behavior on opacity { ... }`) also carries
  `value_source_property` with the property the behavior is attached to.

A plain property binding (`width: parent.width`) is no longer a symbol. The
same evidence stays in the structural facts as `qml.binding.v1`, with the bound
property in the `property_name` metadata key. Read bindings from the fact
table, not from the symbol table.

qmldir type rows use the symbol kind `export`, not `class`, because a qmldir
line exports a type rather than declaring one.

## Other symbol rows

- A signal is an `event` symbol. Its signature is the declaration
  (`signal activated(int index, string name)`) and its metadata carries a
  `parameters` list of `{name, type}` objects.
- A `required property` with no initializer is a `property` symbol with
  `required: true` in its metadata.
- A file that opens with `pragma Singleton` marks its root symbol
  `singleton: true`. The pragma line itself is also a structural fact,
  `qml.pragma.v1`, with the metadata keys `name` and `value`.

## Type facts

- A property records its declared type.
- A function records the type its return annotation states
  (`function build(): Item`). A function with no return annotation records no
  type.
- A typed parameter (`function save(doc: Backend.DocumentModel)`) records its
  annotation.
- A nested object with an `id` records its object type, so `docModel.flush()`
  can resolve through the `docModel` row. The root object's `id` records the
  file's component.

A module qualifier stays in the type name (`Backend.DocumentModel`).

## Test roles

A `TestCase` object is a test container when it is the root object or a
nested object (`Item { TestCase { when: windowShown } }`), bare or
module-qualified (`QtTest.TestCase`). Functions inside a container get the Qt
Quick Test roles: `test_*` and `benchmark_*` are test cases, `init*` is fixture
setup, and `cleanup*` is fixture teardown.

## Call and property resolution

- A call with an id receiver (`root.refresh()`) resolves to the function the
  object with that id declares. A bare call resolves in the enclosing objects,
  from the nearest outward. A same-named function in an unrelated object does
  not block either rule.
- Calls inside property initializers (`readonly property real size:
  Math.max(...)`) are attributed to the enclosing component, like calls in
  plain bindings.
- A pending call's `receiver_type` is set when the receiver is `this` or the
  `id` of any enclosing object. Other ids get no `receiver_type`; their
  declared type fact is the resolution path.
- A `uses` edge to a property needs a receiver this file describes: `this`,
  `parent`, or a same-file id whose object declares the property. A chained
  receiver (`Color.tooltip.background`, `model.item.x`) or an id whose object
  does not declare the property records no edge.

## The reference contract

Every QML type reference is recorded by the terminal segment of its name, so
`QQC2.Button` is the identifier `Button`. The qualifier goes in the identifier
metadata key `receiver` (`QQC2`). Match the terminal name and read the
qualifier from the metadata.

The metadata key `role` says what the reference is:

- `base_type` — the root object's declared base type. The root also emits an
  `extends` relationship to it: a concrete relationship when the base type is a
  symbol in the same file, a structured pending relationship otherwise.
- `attached_type` — an attached property's type (`Layout.fillWidth`,
  `Kirigami.FormData`).
- `signal_handler` — a handler binding (`onClicked`). The row is a
  `member_access` identifier with the handler's target in `receiver`. A dotted
  handler name carries its qualifier instead, so `Keys.onPressed` gives `Keys`.
  An `onXChanged` handler also carries `change_handler: true`.

A reference with no `role` is an ordinary type usage or member access.

### `.qmltypes` files record no base type

A `.qmltypes` file's root is `Module { ... }`, a descriptor of a module rather
than a component that extends something. Its root emits no `base_type`
identifier and no `extends` relationship. Nested rows (`Component`,
`AttachedType`, and the rest) keep their ordinary type usages.

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

cargo build --release --locked -p julie-extract-cli
ARTIFACT="$(mktemp -d)"
./target/release/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

The numbers below were produced by the 3.2.0 branch build of
`julie-extract`, which reports `binary_version` `3.2.0` in its scan report.

The filesystem audit found 179 `.qml` files, one `.qmltypes` file, and five
exact-basename `qmldir` files. The scan report was `status=ok` with
`files_scanned=751`, `files_changed=750`, `files_unsupported=367`,
`files_failed=0`, and empty `errors`. It carried one recoverable warning,
`slow_file_skipped`, for `src/desktoptheme/breeze/widgets/monitor.svg`, which
exceeds the 1,048,576-byte extraction limit. The report's per-file section was
truncated by the CLI contract, so language-specific counts below come from the
SQLite artifact.

| Artifact evidence | `qml` | `qmldir` |
| --- | ---: | ---: |
| Indexed files | 180 (179 `.qml` + 1 `.qmltypes`) | 5 |
| Symbols | 3,784 | 53 |
| Structural facts | 9,886 | 53 |
| Resolved relationships | 235 | 0 |
| Pending relationships | 1,682 | 0 |
| Parse diagnostics | 121 | 10 |

The QML symbol count is lower than the 3.1.3 count of 7,195 because a plain
property binding is no longer a symbol. The same evidence stays in
`structural_facts`, where 6,363 `qml.binding.v1` rows carry it. The object
model supplies 1,551 `field` rows, qmldir supplies 52 `export` rows, and QML
root objects and inline component bodies supply 175 pending `extends` rows
and 6 resolved ones.

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
