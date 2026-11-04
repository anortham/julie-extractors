# Language coverage evidence

## Decision

Capability proof must describe useful extracted rows. A JSON key with a null, empty, malformed, or out-of-source value is not body evidence. A supported body kind needs a valid nonempty span and its normalized body hash from a registered source fixture. Removing the final supporting row must invalidate that claim.

Five current body-kind claims describe bodiless declarations. Move these kinds from `body_spans.supported` to `body_spans.not_applicable` without inventing spans or changing extraction.

| Language/kind | Source-backed reason | Registered evidence |
| --- | --- | --- |
| HTML/import | Resource links and external script references name another resource; they do not declare its body. Embedded script declarations retain their own body kinds. | `fixtures/extraction/html/basic/expected.json` and other registered HTML fixtures |
| CSS/import | `AtRuleExtractor::extract_import` explicitly clears the body fields on a resource-reference statement. | `fixtures/extraction/css/basic/expected.json`, cross-file fixture |
| CSS/variable | `AtRuleExtractor::extract_at_rule` assigns this kind to `charset_statement`, a charset directive with no body. Custom-property registration remains a separate property fact/symbol. | `fixtures/extraction/css/basic/expected.json` |
| qmldir/export | `QmldirExtractor::push_type_symbol` names a type exported from another file. The directive does not declare that file's implementation. | `fixtures/extraction/qmldir/basic/expected.json` |
| qmldir/module | `QmldirExtractor::extract_module` records a module declaration line. A qmldir manifest has no module body. | `fixtures/extraction/qmldir/basic/expected.json` |

HTML kind selection is in `crates/julie-extractors/src/html/types.rs` and `html/scripts.rs`; paired-tag interior bounds are in `html/helpers.rs`. Other implementation pointers are `crates/julie-extractors/src/css/at_rules.rs`, `crates/julie-extractors/src/qmldir/mod.rs`, and the existing shared body inference/normalization contract in `crates/julie-extractors/src/base/body.rs`. These dispositions concern the currently emitted kinds; a future construct with an actual body requires new fixture evidence.

## Applicability gate

Add `--check-applicability` beside `--strict` in the language quality report. It rejects unclassified or conflicting domain states, unsupported advertised proof, unknown languages, and stale fixture registration. `--strict` retains its existing silent-cell and quality-debt meaning. The eleven retained consumer/context gaps stay visible and do not fail strict mode merely because their consumer-owned closure remains open.

Core-domain classifications come from the capability ledger when it accounts for the domain. Keep `types`, `type_argument_usages`, and `pending_relationships` as observed domains with script-local applicability, following the [July capability policy](2026-07-17-capability-ledger-policy.md). Empty output alone never establishes non-applicability.

Go module manifests record dependency/directive facts, and go.sum records checksums. Neither format contains program declarations with generic type applications. `gosum.rs` records no declarations or unresolved edges, only checksum facts.

qmldir exports name types defined in other files; they are not local type declarations or generic applications. Its dependency/import sites use the existing `qmldir.depends.v1` and `qmldir.import.v1` structural-fact contract. The generic relationship/pending/type tables do not duplicate those facts. Their N/A classification does not claim that module dependencies or type exports are absent. The corpus and golden checks must prove those native facts survive.

XML schema type declarations remain `xml.xsd.type.v1` facts and symbols, as defined in `crates/julie-extractors/src/base/structural_fact_registry/xml.rs`. This schema vocabulary is distinct from program TypeInfo/type-argument usages. Classifying the separate generic type/argument tables must preserve the schema facts rather than suppressing them to obtain an empty result.

## Source correction

The bounded review also reproduced `HTML-IMPORT-BODY`: an external `<script src="external.js">inlineText()</script>` received a span/hash for its inline text because the script extractor reused the paired-tag body helper after selecting Import. External resource references must have no declaration body; true inline-script symbols keep their body spans and hashes. Recognize `src` case-insensitively so `SRC` and `SrC` cannot bypass external-script classification or trigger embedded extraction of ignored inline text. This is an extraction-output correction and needs the existing contract-marker/ledger discipline.

## Consequences

No SQLite schema, extraction level, report schema, or production output format changes follow from repairing evidence claims. The pinned corpus and the 630-cell audit must independently verify the native facts, negative controls, and source boundaries before the goal can close.
