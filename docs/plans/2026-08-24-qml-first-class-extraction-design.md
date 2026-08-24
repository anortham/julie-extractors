# QML First-Class Extraction Design

## Outcome

Julie Extractors will publish enough QML, `qmldir`, and `.qmltypes` evidence for downstream tools to understand Qt modules, components, imports, bindings, object instantiations, and Qt Quick Test roles without reparsing source text.

## Product Boundary

- `julie-extract` remains the integration surface; SQLite and JSONL carry the same versioned facts.
- The extractor publishes local syntax and module evidence. It does not resolve a workspace graph.
- QML capability claims require registered golden fixtures and strict capability-matrix evidence.
- Default tests stay fast; real Qt repositories and parser certification remain explicit slow gates.

## Architecture Quality

**Affected modules:** language registration, QML extraction, structural facts, test-role detection, golden fixtures, capability reporting, and xtask test routing.

**Caller-facing interface:** existing artifact symbols, relationships, structured pending relationships, structural facts, test roles, and type facts. `qmldir` is a new language family member; no Rust-only integration surface is introduced.

**Depth/locality check:** grammar-specific traversal stays in `qml/` and `qmldir/`. Shared registries only dispatch by language or register fact schemas.

**Test surface:** narrow QML/qmldir unit tests, per-language golden tests, capability validation, contract tests, and opt-in real-world certification.

**Seams/adapters:** normalized QML imports use the existing import-symbol metadata keys. Object instantiations use structured pending relationships when a local target is unavailable. Structural facts describe syntax but do not duplicate resolver inputs.

**Rejected shortcuts:** treating `qmldir` as plain text, inferring module identity from directory names alone, emitting both concrete and pending instantiation edges for one use, inventing a new test-role vocabulary for `_data` helpers, and marking missing module semantics as not applicable.

**Architecture risk:** medium. The artifact schema already has the required families, but downstream correctness depends on unambiguous metadata and relationship contracts.

## Language Model

### QML source

- Preserve current component, property, signal, function, identifier, type, literal, region, complexity, documentation, and binding evidence.
- Import symbols carry normalized `source`, `alias`, `local_name`, `imported_name`, and `is_namespace` metadata where each field applies.
- Directory imports, URI imports, version qualifiers, aliases, and JavaScript resource imports are distinguishable without parsing display names.
- Root and nested object definitions emit object-instantiation facts. A locally declared component may produce a concrete `instantiates` relationship; otherwise one structured pending `instantiates` relationship carries the target name and import context.
- Embedded JavaScript declarations remain ordinary QML/JavaScript symbols when the grammar gives stable declaration nodes.

### `qmldir`

- Add `tree-sitter-qmldir` from `https://github.com/tree-sitter-grammars/tree-sitter-qmldir` pinned to commit `c57e00865a1a6f1cca83340d6dad91f13df55479`.
- Register the extensionless basename `qmldir` as its own language and extractor.
- Publish the module URI plus component, singleton, internal, JavaScript resource, plugin, classname, typeinfo, depends, import, designer, and prefer declarations supported by the grammar.
- Component declarations expose type name, version, source path, singleton/internal status, and declaration span as typed structural facts and symbols where a named type exists.
- Parser dependency policy, license checks, downstream smoke packaging, and freshness records cover the new git dependency.

### `.qmltypes`

- Register `.qmltypes` under the QML language family and parse its QML-shaped tooling metadata.
- Publish module/type names, exports, prototypes, attached/extension types, properties, signals, methods, parameters, enums, and revision/version data when present.
- Apply the existing extractor input-size ceiling; `.qmltypes` does not get an unbounded generated-file bypass.
- Keep generated declaration detail in symbols, type facts, and structural facts. Do not copy whole source blocks into metadata.

## Qt Quick Test Semantics

- Functions named `test_*` are runnable tests unless the name ends in `_data`.
- `benchmark_*` and `benchmark_once_*` functions are runnable benchmark tests.
- `init`, `cleanup`, `initTestCase`, and `cleanupTestCase` retain lifecycle roles.
- `init_data` and `test_*_data` remain non-test functions; no new public data-provider role is added.
- Filename evidence (`tst_*.qml`) and a `TestCase` container remain part of classification, preventing arbitrary application methods named `test_*` from becoming tests without QML test context.

## Artifact Contracts

- Import symbols are the only source for generic downstream `ImportBinding` creation.
- `qml.import_statement.v1` remains useful pattern evidence and must agree with import-symbol metadata.
- New `qml.object_instantiation.v1`, `qml.typeinfo_type.v1`, and focused child fact kinds are versioned registry entries with bounded typed attributes.
- New `qmldir.*.v1` facts carry module-manifest semantics. Their schemas are contract-tested and represented in capabilities.
- Every fixture proves SQLite/JSONL-equivalent extraction through the existing golden artifact path.

## Testing and Certification

- Replace the single-file QML `cross_file` fixture with a real multi-file module containing `qmldir`, components, aliases, directory imports, and unresolved/external types.
- Add `.qmltypes` and Qt Quick Test fixtures, including `_data` negative controls and both benchmark prefixes.
- Extend `cargo xtask test language qml` so it runs QML unit tests and only the QML-family golden fixtures.
- Add `cargo xtask test language qmldir` with the same narrow behavior.
- Keep real-world Qt corpus checks outside the default suite and record grammar error-rate/coverage evidence before changing parser pins.
- Run `node scripts/language-data-quality-report.mjs --strict`; `silent_cells` and `quality_bar_debts` remain `0`.

## Acceptance Criteria

- [ ] URI, directory, aliased, versioned, and JavaScript QML imports have normalized import-symbol metadata.
- [ ] `qmldir` and `.qmltypes` produce useful module/type rows in registered goldens.
- [ ] Multi-file QML component uses emit one authoritative local or pending instantiation relationship.
- [ ] Qt Quick Test functions, data helpers, lifecycle methods, and benchmarks are classified correctly.
- [ ] QML capabilities contain no unsupported positive claim and no implementation gap mislabeled `not_applicable`.
- [ ] Per-language QML/qmldir commands include their golden fixtures and remain fast.
- [ ] Default, golden, capability, contract, strict-quality, real-world, and Windows gates pass at their required tiers.

## Primary References

- Qt QML module manifests: <https://doc.qt.io/qt-6/qtqml-modules-qmldir.html>
- Qt QML CMake modules: <https://doc.qt.io/qt-6/qt-add-qml-module.html>
- Qt Quick Test: <https://doc.qt.io/qt-6/qtquicktest-index.html>
- Qt Quick Test `TestCase`: <https://doc.qt.io/qt-6/qml-qttest-testcase.html>
