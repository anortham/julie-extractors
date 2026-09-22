# Qt first-class gap closure (QML, qmldir, Qt JavaScript, Qt C++)

Date: 2026-09-21. Status: draft, waiting for approval. Revision 2 after a
Codex review the same day. Written from the code-kb audit in
`~/source/code-kb/docs/plans/025-qt-qml-first-class-support.md`, which holds
the evidence, the pinned corpus commits, and the code-kb side of each fix.
This file lists only the extractor tasks with their file pointers.

## Outcome

A Qt developer's everyday questions get answers from the artifact alone:
which components extend X, who reads singleton Y, who handles signal Z, what
does this component look like as a tree, and what does this Qt C++ header
expose to QML.

## Release 3.2.0: QML, qmldir, Qt JavaScript

Grammar: no pin change. `tree-sitter-qmljs` already exposes every node used
below (`ui_inline_component`, `ui_pragma`, `ui_required`,
`ui_signal_parameters`, `ui_object_definition_binding`, `nested_identifier`).

### Symbols (`qml/mod.rs`, `traverse_node`)

1. **Nested objects.** A non-root `ui_object_definition` and every
   `ui_object_definition_binding` (`Behavior on color { }`) emits a symbol:
   name = the `id` binding when present (reuse `object_id_binding` in
   `qml/relationships.rs`), else the `type_name` text; kind `Field` (code-kb
   never skips it, renders it as a container only when it has children, and
   `is_js_parameter_or_local` in `qml/identifiers.rs` does not exclude it from
   containment); signature `t: Timer` or `Rectangle` (code-kb cuts skeleton
   signatures at `{`); parent = the enclosing object or inline component;
   metadata `object_type`, `binding_kind: "object"`, and
   `value_source_property` for the `on` form. Keep the root component's `id:`
   property row (`qml/mod.rs:82` area); drop the separate `id:` row for nested
   objects. `receiver_can_resolve_locally` in `qml/relationships.rs:130`
   expects an id as a `Property` with an `id:` signature; teach it the object
   row. Relationship and pending `from_symbol` values stay the enclosing
   component or function, never the object row (code-kb's impact walk skips
   edges from `field` rows).
2. **Plain property bindings** (`binding_kind: property_binding`) stop being
   symbols. `qml.binding.v1` facts keep them (`base/code_structural_facts.rs:382`).
   Signal handlers stay `Function` symbols; declared properties stay
   `Property` symbols. Consequences to carry: property-target resolution in
   `qml/relationships.rs:355` re-anchors to the enclosing object;
   `tests/qml/bindings.rs`, the capability matrix, and every QML golden change;
   binding text leaves symbol search. Owner decision.
3. **Inline components.** `ui_inline_component` emits a `Class` symbol from
   the `name` field, signature `component Name: Base`, metadata
   `base_types: [Base]`, and its `component` object's children parent to it.
4. **Signals.** `ui_signal` sets the signature to the source text
   (`signal closeRequested(string reason)`) and records `parameters`
   (name, type) in metadata from `ui_signal_parameters`.
5. **Pragmas.** `ui_pragma` with name `Singleton` sets `singleton: true` on
   the root class and prefixes its signature with `singleton `. Every pragma
   emits a `qml.pragma.v1` structural fact with `name` and `value`
   (`ComponentBehavior` / `Bound`).
6. **Bare `required` declarations.** `ui_required` emits a `Property` symbol
   named from its `name` field with metadata `required: true`,
   `inherited: true` (plasma-workspace `DeviceItem.qml:23`).
7. **Multi-line property values.** When a `ui_property` value spans lines,
   the signature is the declaration head only (`property var shellValues`).

### Identifiers and references (`qml/identifiers.rs`, `qml/relationships.rs`)

One contract, tested end to end with code-kb `refs`, `context`, and
`blast_radius` (positive and negative cases).

8. **Identifier metadata transport.** Add an optional metadata map to the
   extractor `Identifier` (`base/types.rs:320`) and merge it in
   `map_identifiers` (`julie-extract-cli/src/extraction.rs:688`), which today
   builds metadata from the source receiver only. SQLite `metadata_json` and
   JSONL already carry it; no schema change. Run
   `receiver_before_identifier` for `type_usage` rows too, so a qualified type
   usage gets `receiver` like a call does.
9. **Qualified type names.** For a `nested_identifier` type name the
   identifier name is the terminal segment (`Page`); the receiver comes from
   task 8 (`Kirigami`). The pending `instantiates` rows already do this
   (`qml_component_target`, `qml/relationships.rs:238`); keep both in step.
10. **Root base type.** The root `ui_object_definition` emits a `TypeUsage`
    identifier for its type name with metadata `role: "base_type"` (today only
    nested objects do), and a structured pending relationship of kind
    `extends` from the component to the base type with terminal name,
    receiver, and import context (same shape as `instantiates`). A local
    inline component or same-file class resolves concretely as it does for
    instantiation.
11. **Attached binding names.** A `ui_binding` whose name is a
    `nested_identifier` with a capitalized head (`Layout.fillWidth`,
    `Keys.onPressed`, `Kirigami.FormData.label`) emits a `TypeUsage`
    identifier for the attached type in the task 9 shape (`Layout`, or
    `FormData` with receiver `Kirigami`).
12. **Handler to signal.** At an `on<Signal>` binding and at
    `function on<Signal>()` inside a `Connections` object (the function form
    lacks `handled_signal` metadata today, `qml/mod.rs:284` area), record
    `handled_signal` and emit a `MemberAccess` identifier named after the
    signal with metadata `role: "signal_handler"` and a receiver: the
    `Connections.target` id when it is an id, else the enclosing object's
    type, else none. `on<X>Changed` handlers point at property `X` with
    `change_handler: true`. `semantics::handled_signal_from_binding_name`
    already derives the name. Unowned matches are candidates; code-kb labels
    them.

### qmldir (`qmldir/mod.rs`, `push_type_symbol`)

13. Object type, singleton, and internal rows change kind from `Class` to
    `Export` (`base/kinds.rs:99`), keeping `version`, `file`, and `singleton`
    metadata. The module row stays `Module`. Lookup lists still show the
    export next to the class; that is intended.

### Qt JavaScript (`javascript/`, `pipeline.rs`, `syntax/mod.rs`)

14. One shared pre-pass, called by both `parse_for_language`
    (`pipeline.rs:283`) and the check path (`syntax/mod.rs:124`), recognizes
    `.pragma` and `.import` directive lines at the top of a `.js` file
    (comments and blank lines allowed before them), replaces each with
    same-length spaces so byte spans hold, and parses the result while the
    original text stays the source of record. Record each directive as an
    `Import` symbol (`.import QtQuick 2.0 as QQ`, `.import "file.js" as F`) or
    a `javascript.qml_directive.v1` fact (`.pragma library`); the registry
    adapter (`registry.rs:82`) must return those facts. `julie-extract check`
    accepts the file. Tests: CRLF, UTF-8, a directive after a comment, a
    non-directive line that starts with a dot.

### Tests, goldens, capabilities, contracts, docs

15. `fixtures/qml`: an inline component, a `pragma Singleton` service, a file
    with qualified instantiations and a same-named base (`BarWidget.qml`
    extending `BarWidget`), a `Behavior on`, attached bindings, a
    `Connections` handler in both forms, an `onXChanged` handler, a bare
    `required`, and a `.pragma library` JS file; a generated `.qmltypes` near
    the 1 MiB limit (`julie-extract-cli/src/limits.rs:13`). Regenerate goldens.
16. Capability matrix: `qml.pragma.v1`, `javascript.qml_directive.v1`, the
    `export` kind for qmldir, the `extends` pending kind, and the `base_type`,
    `signal_handler`, and attached-type identifier roles.
17. `docs/languages/qml.md`: document the object-symbol model, the reference
    contract, and the handler link. Run `cargo xtask test language qml` and
    `cargo xtask test language qmldir`.
18. Classify every output change per
    `docs/contracts/extraction-output-changes.md` (binding symbols disappear,
    qmldir kinds change, new pending kind, identifier metadata) and advance
    the extraction identity epoch (`lib.rs`, currently 10). Review the public
    extractor API change from task 8.

## Release 3.3.0: Qt C++ macros (`cpp/`)

Evidence: Kirigami 981 parse diagnostics in 70 of 102 C++ files,
plasma-workspace 4,502 in 636 of 1,030; 297 and 914 `Q_PROPERTY` lines yield 0
property symbols; `Q_SIGNALS:` yields an empty-name field; methods after a
macro lose return types and gain a false `override`; constructors emit twice;
forward declarations are `Class` rows. Not every diagnostic is macro-caused;
measure after the pre-pass before patching downstream symptoms.

19. **Macro pre-pass.** A token-level rewrite shared by extraction and
    `check` (same mechanism as task 14) that keeps every byte position and
    newline: `Q_OBJECT`, `Q_GADGET`, `QML_ELEMENT`, `QML_SINGLETON`,
    `QML_ANONYMOUS`, `Q_INVOKABLE`, and `*_EXPORT` between `class` and the
    name become spaces; `Q_PROPERTY(...)`, `Q_ENUM(...)`,
    `QML_NAMED_ELEMENT(...)`, `QML_UNCREATABLE(...)`, `QML_ATTACHED(...)`
    become spaces after their balanced argument text (multi-line allowed) is
    captured; `Q_SIGNALS`, `Q_SLOTS`, `signals`, `slots` are recognized as
    section markers in both the bare (`Q_SIGNALS:`) and prefixed
    (`public Q_SLOTS:`, `columnview.h:627`) forms without rewriting the
    existing access label. Strings and comments are never rewritten.

    Status: done in 3.3.0, commits `3a28fde8`, `ebc09b77`, `7d2be7f3`,
    `a0be6571`, `2e54a1b2`, `9e466709`. `cpp/qt_macros.rs` blanks four rule
    kinds on the scan path, the `check` path, and the `.h` header probe, with
    the byte length and every newline preserved.
20. **Qt facts and symbols.** Each captured `Q_PROPERTY(type name READ r
    WRITE w NOTIFY n ...)` emits a `Property` symbol under the class with
    metadata `read`, `write`, `notify`, `member`, `constant`, `final` and a
    `cpp.qt_property.v1` fact. Methods in a signals section become `Event`
    symbols; slots get `qt_slot: true`; `Q_INVOKABLE` methods get
    `qt_invokable: true`; `QML_NAMED_ELEMENT(X)` and `QML_ELEMENT` set
    `qml_element` on the class.

    Status: done in 3.3.0, commits `37d9d9db`, `7d2be7f3`, `9e466709`. Kirigami
    yields 297 `property` rows, 297 `cpp.qt_property.v1` facts, and 196 `event`
    rows; plasma-workspace yields 920, 920, and 1,060.
21. **Existing C++ defects on the same headers, re-measured after task 19:**
    false `override` prefix and lost return types (`cpp/signatures.rs`),
    duplicate constructor and destructor rows (same declaration extracted
    twice, not overloads or declaration/definition pairs), and forward
    declarations (`class X;`), which stop being emitted as symbols.

    Status: done in 3.3.0, commits `9f256e54`, `b593492e`. All three counters
    are 0 on both corpora: `override `-prefixed signatures fall from 412 to 0
    on Kirigami and 2,365 to 0 on plasma, forward-declaration rows from 57 and
    653 to 0, duplicate constructor rows from 43 and 493 to 0.
22. Goldens: a Kirigami-style header (`Q_OBJECT`, `Q_PROPERTY`, `Q_SIGNALS`,
    `public Q_SLOTS:`, `Q_INVOKABLE`, `QML_ELEMENT`, export macro, forward
    declaration). Acceptance on Kirigami `src/layouts/columnview.h` at
    `ca7d636`: zero parse diagnostics, 38 property symbols, signals as events,
    one row per constructor, no forward-declaration rows. Add
    `docs/languages/cpp-qt.md`. Advance the epoch again.

    Status: done in 3.3.0, commits `3d05f3d5`, `7f0b18ff`, `1650681a`,
    `43e2e149`. `columnview.h` reports 0 parse diagnostics (was 92), 38
    property rows, 37 events, one row per declared constructor and destructor,
    and no forward-declaration row. The epoch was **not** advanced: it stays 10,
    and `EXTRACTION_CONTRACT_VERSION` gains the `qt-cpp-v1` suffix instead,
    because the contract version is what consumers observe and no symbol id of
    an unchanged non-Qt file moves.

## Effort (agent sessions)

3.2.0: 4 to 5 sessions (task 8 to 12 is the largest block). 3.3.0 took 4
sessions against the 2-to-3 estimate: waves 6 and 7 built the pre-pass and the
symbols, wave 8 closed a review round, and wave 9 did the contracts, docs, and
release. Human time: approval of this plan, the decision on task 2, and the
two release approvals. Each julie release needs a code-kb release after it
(pin bump with all six checksums) before users see the change.

## Review record

Codex reviewed revision 1 on 2026-09-21. Folded in: `Variable` rows are
skipped by the code-kb skeleton and excluded from identifier containment
(task 1 kind), signatures are cut at `{` (task 1), root id would vanish (task
1), binding removal consequences (task 2), identifier metadata transport
(task 8), pending rows already split qualified names (task 9), `blast_radius`
needs a relationship not an identifier (task 10), name-only handler links
overclaim (task 12), preprocessing must reach the check path (tasks 14, 19),
`slots:`/`public:` length mismatch and `public Q_SLOTS:` (task 19), 38 not 17
`Q_PROPERTY` lines (task 22), epoch advance for both releases (tasks 18, 22).
Added from the review: bare `required` (task 6), `Behavior on` (task 1),
attached binding names (task 11), `onXChanged` (task 12), large `.qmltypes`
fixture (task 15). Dropped: the capitalized-receiver identifier rule; code-kb
already stores the source receiver on member-access rows and matches it
instead.
