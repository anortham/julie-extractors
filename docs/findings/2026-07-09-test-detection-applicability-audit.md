# Test-detection applicability audit

Date: 2026-07-09

## Scope and method

This audit classifies `test_case`, `test_container`, and `test_lifecycle` for
HTML, CSS, SQL, regex, Markdown, JSON, TOML, and YAML. Empty extraction is not
used as applicability evidence.

The evidence order was:

1. Resolve the exact parser versions pinned in
   `crates/julie-extractors/Cargo.toml:31-66` with `cargo metadata --locked`.
2. Inspect each resolved parser's complete named-node inventory in its
   generated `src/node-types.json` (both block and inline inventories for
   `tree-sitter-md`).
3. Inspect the language-local extractor dispatch that defines this product's
   supported semantic surface.
4. Scan every registered expected artifact for the eight capability rows for
   `is_test=true`, `test_container=true`, or `test_lifecycle=true`.

The resolved grammar inventory contained 19 named HTML nodes, 64 CSS nodes,
523 SQL nodes, 38 regex nodes, 51 Markdown block nodes plus 26 inline nodes, 13
JSON nodes, 19 TOML nodes, and 36 YAML nodes. A full-name audit found no
grammar-defined test/suite/setup/teardown/hook/fixture node in any inventory;
SQL's `keyword_before`, `keyword_after`, and `window_specification` matches are
ordinary query-window syntax, not test roles.

The registered-artifact scan found no positive test-role field in any of the
21 registered fixtures across these eight languages. That only rules out a
`supported` classification; it does not justify `not_applicable`.

## Classification rule

- `supported`: a registered golden emits the role.
- `not_applicable`: the language and pinned grammar genuinely lack the role;
  recognizing it would require a different host language or harness rather
  than interpreting this language's own structures.
- `open_gaps`: the language can carry a test framework's conventions, but the
  meaning comes from an external framework, schema, embedded language, or file
  convention and no registered golden proves a stable detector.

## Classification ledger

| Language | `test_case` | `test_container` | `test_lifecycle` |
| --- | --- | --- | --- |
| HTML | `open_gaps` | `open_gaps` | `open_gaps` |
| CSS | `not_applicable` | `not_applicable` | `not_applicable` |
| SQL | `open_gaps` | `open_gaps` | `open_gaps` |
| regex | `not_applicable` | `not_applicable` | `not_applicable` |
| Markdown | `open_gaps` | `open_gaps` | `open_gaps` |
| JSON | `open_gaps` | `open_gaps` | `open_gaps` |
| TOML | `open_gaps` | `open_gaps` | `open_gaps` |
| YAML | `open_gaps` | `open_gaps` | `open_gaps` |

No role is classified as `supported` because none has registered golden
evidence.

## Per-language findings

### HTML: framework-defined, keep all roles open

`tree-sitter-html` 0.23.2 supplies document, element, attribute, script/style,
doctype, comment, and text structure, not test-role nodes. The product dispatch
in `crates/julie-extractors/src/html/mod.rs:105-130` handles those same grammar
categories. Element extraction preserves arbitrary attributes
(`crates/julie-extractors/src/html/elements.rs:68-135`), and inline JavaScript
is delegated to the JavaScript extractor
(`crates/julie-extractors/src/html/scripts.rs:18-108`). Therefore an HTML file
can carry framework-defined cases, suites, or lifecycle hooks through markup or
embedded JavaScript even though HTML has no native test declaration. Each role
remains open until one stable convention is selected, protected with nearby
negative controls, and proven by a registered HTML golden.

### CSS: genuinely absent, all roles not applicable

The complete `tree-sitter-css` 0.25.0 inventory models stylesheets, selectors,
declarations, values, imports/namespaces, and at-rules. The product visitor
enumerates rule sets, imports and other at-rules, keyframes, media/supports
rules, and custom properties (`crates/julie-extractors/src/css/mod.rs:51-153`).
None of these constructs declares an executable test case, groups test cases,
or defines setup/teardown behavior. A CSS asset can be input to a test owned by
an HTML or executable-language harness, but CSS itself does not encode any of
the three roles. All three roles are `not_applicable`.

### SQL: framework-defined, keep all roles open

`tree-sitter-sequel` 0.3.11 models SQL query, DDL, transaction, schema, trigger,
and routine syntax; it has no native test-role node. The product exposes tables,
procedures/functions, views, indexes, triggers, CTEs, schemas, aliases, and
other SQL structures (`crates/julie-extractors/src/sql/mod.rs:264-410`). Those
real language constructs can be assigned case, container, and setup/teardown
meaning by a SQL testing framework or project naming convention. Because that
meaning is external rather than absent, all three roles stay open until named
framework contracts, false-positive controls, and registered goldens define a
safe supported surface.

### regex: genuinely absent, all roles not applicable

The complete `tree-sitter-regex` 0.25.0 inventory contains pattern, group,
character-class, assertion, quantifier, alternation, escape, backreference,
Unicode-property, and conditional syntax. The product visitor mirrors that
surface (`crates/julie-extractors/src/regex/mod.rs:50-180`). A regex can be test
input or an assertion value in a host test framework, but regex syntax cannot
declare a test case, suite/container, or lifecycle hook. Those roles belong to
the host language or data schema, so all three are `not_applicable`.

### Markdown: framework-defined, keep all roles open

The two `tree-sitter-md` 0.5.3 inventories model document sections, headings,
fenced code, links, frontmatter, and inline formatting, not native test roles.
The product extracts sections, fenced code, links, and YAML/TOML frontmatter
(`crates/julie-extractors/src/markdown/mod.rs:73-94` and
`crates/julie-extractors/src/markdown/mod.rs:108-146`). Documentation-test tools
can assign case, grouping, or setup/teardown meaning to those structures. That
meaning is tool-specific, so all three roles remain open pending one named
contract with negative controls and a registered Markdown golden.

### JSON: schema-defined, keep all roles open

`tree-sitter-json` 0.24.8 models documents, objects, arrays, pairs, and scalar
values. The product intentionally materializes key/value pairs
(`crates/julie-extractors/src/json/mod.rs:70-154`). Arbitrary external schemas
can use those objects, arrays, and keys to represent cases, suites, and
lifecycle actions. The grammar cannot distinguish those schemas itself, so all
three roles remain open until a named schema is selected and fixture-proven.

### TOML: schema-defined, keep all roles open

`tree-sitter-toml-ng` 0.7.0 models tables, array tables, pairs, and scalar/array
values. The product materializes tables and pairs
(`crates/julie-extractors/src/toml/mod.rs:71-151`). External tool schemas can
assign test-case, container, or lifecycle meaning to table and key names; TOML
does not define that meaning. All three roles remain open until a named schema,
negative controls, and a registered golden establish safe support.

### YAML: schema-defined, keep all roles open

`tree-sitter-yaml` 0.7.2 models streams/documents, mappings, sequences, scalar
values, tags, anchors, and aliases. The product materializes block mapping
pairs and preserves anchors (`crates/julie-extractors/src/yaml/mod.rs:77-182`).
External schemas can use mappings and sequences for test cases, grouped suites,
and setup/teardown actions. Since YAML itself assigns none of those roles, all
three remain open until a named schema is selected and fixture-proven with
nearby negative controls.

## Closure policy

Open roles are deliberate product debt, not implied support. A later closure
must name the framework/schema contract, keep recognition language-local, add
similar non-test controls, register a golden, and only then move the role to
`supported`. CSS and regex can be reconsidered only if their language grammar
or product boundary gains a native role construct; host-language conventions
do not change their applicability classification.
