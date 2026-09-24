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

A qmldir `#` comment is a `comment` source region. A `.mjs` resource line is a
`qmldir.javascript_resource.v1` fact, like `.js`. The `static` and `system`
directives are `qmldir.static.v1` and `qmldir.system.v1` facts.

## Other symbol rows

- A signal is an `event` symbol. Its signature is the declaration
  (`signal activated(int index, string name)`) and its metadata carries a
  `parameters` list of `{name, type}` objects.
- A signal is `public` and takes the doc comment above it.
- A property modifier is a metadata key: `required: true`, `readonly: true`,
  or `default: true`. The `qml.property_declaration.v1` fact carries the same
  keys. A property body span covers the value, not the declaration. A
  property whose value is a literal has no body span.
- A property whose value is an object (`property QtObject wifi: QtObject
  {}`) owns that object. The object is a `field` with `bound_property` set to
  the property name. An object bound to a plain property
  (`background: Rectangle {}`) carries `bound_property` too.
- An enum member records its initializer in the metadata key `value`: an
  integer when the initializer is a number, else the source text.
- A signal handler binding (`onClicked: ...`) is a `function` symbol. Its
  signature is the header only (`onClicked`, or `onPressed: (mouse) =>`). Its
  body span covers the handler value. The metadata key `handled_signal` names
  the signal (`clicked`).
- A function that `signal.connect(fn)` connects also gets `handled_signal`.
- A function nested in another function is `private`.
- A file that opens with `pragma Singleton` marks its root symbol
  `singleton: true`. The pragma line itself is also a structural fact,
  `qml.pragma.v1`, with the metadata keys `name` and `value`.

## Type facts

- A property records its declared type.
- A function records the type its return annotation states
  (`function build(): Item`). A function with no return annotation records no
  type.
- A typed parameter (`function save(doc: Backend.DocumentModel)`) or local
  (`let doc: DocumentModel = pick()`) records its annotation.
- A function local with no annotation gets an inferred type fact from its
  initializer: `new T()`, or a call to a same-file function with a return
  annotation, named bare (`load()`) or through a same-file id
  (`root.load()`). A bare call records a type only when the scope object (the
  nearest enclosing object) declares the callee or is the root object of its
  component. Any other scope object also has the members of its type (a Qt
  type, a type from another file, or a same-file inline component), and one
  of them can shadow the name at runtime, so such a call records nothing.
  Use an id (`root.load()`) for a typed call from a nested object. A
  `void` return, a signal, a callee that only an unrelated object declares,
  a name that an object between the scope object and the root declares, a
  parameter, local, nested function, loop, or catch binding that shadows the
  name, an optional call, a chain, or a callee in another file records no
  fact.
- A nested object with an `id` records its object type, so `docModel.flush()`
  can resolve through the `docModel` row. The root object's `id` records the
  file's component.

A module qualifier stays in the type name (`Backend.DocumentModel`).

## Annotations, doc comments, and component URLs

- A Qt annotation (`@Deprecated { reason: "old" }`) goes into the
  `annotations` of the object, property, signal, or function it precedes. The
  annotation name is also a `type_usage` identifier with `role: annotation`.
  The annotation's own bindings are not `qml.binding.v1` facts.
- A doc comment is a `/** */`, `/*! */`, or `///` comment directly above the
  declaration or above its annotations.
- A string that names a `.qml` file is a `qml.component_url.v1` fact when it
  is a call argument (`Qt.resolvedUrl("pages/Home.qml")`,
  `Qt.createComponent("Dialog.qml")`) or a direct binding value
  (`initialItem: "pages/Home.qml"`). The metadata keys are `url` and
  `carrier`, which holds the callee or the binding name.
- A grouped binding (`font { bold: true }`) is not an object. Its bindings
  are `qml.binding.v1` facts with the qualified name `font.bold`.
- `qml.object_instantiation.v1` also covers value sources
  (`Behavior on opacity {}`).
- An `import "lib.mjs" as Lib` line has `import_kind: javascript`, like
  `.js`.

## Test roles

A `TestCase` object is a test container when it is the root object or a
nested object (`Item { TestCase { when: windowShown } }`), bare or
module-qualified (`QtTest.TestCase`). Functions inside a container get the Qt
Quick Test roles: `test_*` and `benchmark_*` are test cases, `init*` is fixture
setup, and `cleanup*` is fixture teardown.

## Call and property resolution

- A call with an id receiver (`root.refresh()`) resolves to the function the
  object with that id declares. A bare call resolves in the scope object (the
  nearest enclosing object) and then the root object of its component, as
  Qt's scope rules say. Objects in between are not in scope. When one of
  them declares the name, scope resolution stops, because that object may be
  the root of an implicit component such as a delegate. A same-named
  function in an unrelated object does not block either rule. A bare name
  that scope does not resolve falls back to the one visible same-file
  function or signal of that name. These edges do not check the members of
  the scope object's type, so a Qt, other-file, or inline-component member
  with the same name can shadow the target at runtime. Local type inference
  does not use a bare call from such an object.
- A call inside a signal handler belongs to the handler `function` symbol.
  A call inside a function belongs to that function, also when the function
  is nested. Calls inside property initializers (`readonly property real
  size: Math.max(...)`) and plain bindings belong to the enclosing object.
- A pending call through an import alias (`Utils.clamp()` with
  `import "utils.js" as Utils`) sets `import_context` to the import source.
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
  The signal segment of `root.refreshed.connect(fn)` also has this role,
  with `receiver` set to `root`.
- `annotation` — a Qt annotation name (`@Designer`).
- `value_source_target` — the property a value source targets
  (`Behavior on opacity` gives `opacity`). The `receiver` is the enclosing
  object type.

A reference with no `role` is an ordinary type usage or member access.

### `.qmltypes` files record no base type

A `.qmltypes` file's root is `Module { ... }`, a descriptor of a module rather
than a component that extends something. Its root emits no `base_type`
identifier and no `extends` relationship. Nested rows (`Component`,
`AttachedType`, and the rest) keep their ordinary type usages.

Inside the module descriptor:

- A `Component` with a `prototype` emits a pending `extends` relationship to
  the prototype.
- Each name in `exports` is an `export` symbol. The metadata keys are
  `module` and `versions`.
- A `Property` or `Parameter` records its `type` as a type fact. A `Method`
  records its `returnType`, or its `type` when there is no `returnType`.
- An `Enum` takes its values from the Qt 6 list form and the Qt 5 object
  form.

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
