# Test-role contract closure (2026-08-20)

## Decision

Close the 28 legacy `test_detection` gaps against named language or tool
contracts. A role is `supported` only when its contract is implemented and a
registered golden emits the role. A role is `not_applicable` only when the
adopted contract has no such role; it is not inferred from an empty extraction.
Every row below has `open_gaps: []` in
`fixtures/extraction/capabilities.json`.

| Language | Named contract and evidence | `test_case` | `test_container` | `test_lifecycle` |
| --- | --- | --- | --- | --- |
| Rust | `#[cfg(test)] mod` container plus Rust test attributes in `rust:test_roles` | supported | supported | not applicable |
| C | Criterion `TestSuite`/`Test` macros and suite/test init/fini hooks in `c:test_roles` | supported | supported | supported |
| C++ | Catch2 `TEST_CASE`/`SECTION` and GoogleTest fixture hooks in `cpp:test_roles` | supported | supported | supported |
| Zig | Zig `test` declarations in `zig:test_roles` | supported | not applicable | not applicable |
| HTML | Browser Mocha document marker plus BDD calls in `html:test_roles` | supported | supported | supported |
| SQL | pgTAP runner, schema, routine, and setup/teardown naming contract in `sql:test_roles` | supported | supported | supported |
| Markdown | rustdoc executable Rust/unspecified fences in `markdown:test_roles` | supported | not applicable | not applicable |
| JSON | JSON Schema Test Suite group/case shape in `json:test_roles` | supported | supported | not applicable |
| TOML | trycmd case shape and nextest group/setup tables in `toml:trycmd_roles` and `toml:nextest_roles` | supported | supported | supported |
| YAML | Google container-structure-test v2 command-test shape in `yaml:test_roles` | supported | supported | supported |
| XML | Apache Ant project/target/junit/test chain in `xml:test_roles` | supported | supported | not applicable |

The `not_applicable` cells are contract-level conclusions. Rust's `cfg(test)`
and Zig's `test` declarations provide no adopted lifecycle or suite syntax;
rustdoc treats executable fences as examples, not suites or hooks; JSON Schema
Test Suite groups cases but defines no setup/teardown role; and Ant's JUnit
task contract defines targets and tests but no lifecycle symbol. Ordinary
helpers, headings, nested lookalikes, and arbitrary keys are therefore not
promoted into roles. This is distinct from the supported cells, where the
fixture contains positive role metadata and nearby controls that must remain
unclassified.

## Named contracts and primary sources

- Rust: the [Rust Reference `cfg` attribute](https://doc.rust-lang.org/reference/conditional-compilation.html#the-cfg-attribute) defines the built-in `test` predicate; [rustdoc documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html) define executable Rust examples and the default language for unspecified fences.
- C: [Criterion features and test suites](https://criterion.readthedocs.io/en/master/features.html) define the `TestSuite`/`Test` and init/fini vocabulary used by the fixture.
- C++: [Catch2 test cases and sections](https://github.com/catchorg/Catch2/blob/devel/docs/test-cases-and-sections.md) define `TEST_CASE`/`SECTION`; [GoogleTest testing reference](https://google.github.io/googletest/reference/testing.html) defines fixture setup and teardown methods.
- Zig: the official [Zig language reference](https://ziglang.org/documentation/master/#test) defines `test` declarations; it does not define a suite or lifecycle role for this extractor contract.
- HTML: Mocha's [browser runner](https://mochajs.org/running/browsers/), [BDD interface](https://mochajs.org/interfaces/bdd/), and [hooks](https://mochajs.org/features/hooks/) define the required `mocha.js`/BDD marker and `describe`/`context`/`it`/hook calls.
- SQL: [pgTAP](https://pgtap.org/) documents the `runtests`/`do_tap` runner and TAP-returning test routines used by the fixture.
- Markdown: [CommonMark fenced code blocks](https://spec.commonmark.org/current/#fenced-code-blocks) provide the host syntax; [rustdoc documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html) provide the executable-fence contract.
- JSON: the [JSON Schema Test Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite) defines schema groups with `description`, `schema`, and `tests`, and case objects with `description`, `data`, and `valid`; JSON itself has no test-role semantics (see the [JSON Schema data model](https://json-schema.org/understanding-json-schema/basics)).
- TOML: [trycmd](https://github.com/assert-rs/trycmd) defines command-case TOML; [cargo-nextest configuration](https://nexte.st/docs/configuration/) defines test groups and setup scripts. These are tool schemas layered on TOML, not TOML-native roles.
- YAML: Google's [container-structure-test command tests](https://github.com/GoogleContainerTools/container-structure-test/blob/main/docs/tests/command_test.md) define the v2 `schemaVersion`, `commandTests`, `name`, `setup`, and `teardown` contract.
- XML: Apache Ant's [JUnit task](https://ant.apache.org/manual/Tasks/junit.html) defines `<project>` targets containing `<junit>` and nested `<test>` elements; report XML and lookalike tags are outside that contract.

## Registered evidence and controls

The reconciliation registers these new goldens in the capability matrix:

`html/test_roles`, `json/test_roles`, `markdown/test_roles`, `sql/test_roles`,
`toml/trycmd_roles`, `toml/nextest_roles`, `yaml/test_roles`, and
`xml/test_roles`. Existing `rust/test_roles`, `c/test_roles`,
`cpp/test_roles`, and `zig/test_roles` remain registered and are included in
the matrix above.

The fixtures retain false-positive controls: qualified/member Mocha calls and
documents missing the Mocha marker; missing pgTAP runners and schemas; rustdoc `ignore` and non-Rust
fences; malformed or nested JSON lookalikes; incomplete trycmd and unmarked
nextest tables; YAML keys outside direct v2 command tests; and Ant report,
outside-target, id-only, and non-JUnit XML shapes. These controls establish
that `not_applicable` and unmarked symbols are deliberate contract boundaries,
not artifacts of missing extraction.
