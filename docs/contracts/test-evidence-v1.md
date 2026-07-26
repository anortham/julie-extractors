# Test Evidence v1

## Scope

This contract describes test-role evidence emitted by `julie-extractors`. It
does not define test runner discovery, execution, scheduling, results, or impact
verdicts.

The public capability object is available in these equivalent forms:

- SQLite: `language_capabilities.kind_coverage_json.test_detection`
- JSONL: `language_capability.kind_coverage.test_detection`
- CLI: `julie-extract languages --json` under
  `languages.languages[].kind_coverage.test_detection`

The object remains additive inside the existing `kind_coverage` value. Current
artifacts use SQLite schema v5, JSONL v4, and extraction contract v4.

## Vocabulary And Emitted Roles

The fixed capability units map to the only emitted classification fields:

| Capability unit | Positive role evidence |
| --- | --- |
| `test_case` | a `symbols` row with `is_test = 1` and `test_lifecycle = 0` |
| `test_container` | a `symbols` row with `test_container = 1` |
| `test_lifecycle` | a `symbols` row with `test_lifecycle = 1` |

`is_test = 1` alone is not `test_case` evidence: sqlite-schema-v4 requires
lifecycle hooks to also set `is_test = 1`, so consumers counting test cases
must exclude rows where `test_lifecycle = 1`.

A true role column is positive evidence for that symbol. Consumers must not
create a second classifier from names, paths, annotations, framework guesses,
or runner configuration.

## Capability Evidence

Each language publishes `supported`, `not_applicable`, and `open_gaps` under
`kind_coverage.test_detection`:

- `supported` means registered golden expected artifacts prove that the mapped
  role column is emitted.
- `not_applicable` means source verification established that the language
  genuinely lacks that role. Failure to detect a role is not enough.
- `open_gaps` means the role is not yet proven. Each gap names the missing
  evidence and its closure task. Gap closures must first determine
  language-native applicability; only roles that exist for the language then
  require registered golden proof. Do not treat an open gap as proof that the
  language has the construct.

Capability evidence describes what the extractor is proven to emit. It is not a
test runner inventory and does not prove that every runnable test was found.

## Producer Quality Gates

`test_detection` is part of the strict code-language quality bar. Every code
language must have at least one golden-backed `supported` role or a
source-backed `not_applicable` role. A cell containing only `open_gaps` is
classified but unresolved and fails the strict report.

The domain-language set is CSS, HTML, JSON, Markdown, regex, SQL, TOML, and
YAML. Those rows are governed by the same fixed vocabulary, exact-once
classification, open-gap ownership, and golden-bidirectionality checks, but are
not required to resolve a role that belongs to an external host, framework, or
schema. The applicability audit remains the authority for negative claims.

The capability gate enforces three independent invariants:

1. Each language classifies `test_case`, `test_container`, and
   `test_lifecycle` exactly once across `supported`, `not_applicable`, and
   `open_gaps`.
2. Every `supported` role is emitted by a registered golden, and every role
   emitted by a registered golden is advertised as supported.
3. Every code language resolves at least one role through `supported` or
   `not_applicable`; remaining role variants may stay as explicit, owned gaps.

Vue `<script>` and `<script setup>` sections use the shared JavaScript and
TypeScript call-style vocabulary. Role symbols are remapped to host-SFC
positions and container parents before publication. Ordinary declarations and
qualified member calls remain negative controls.

## Consumer Gates

Positive role rows remain usable evidence. Before making a negative claim from
the absence of a role, a consumer must establish all of the following:

1. The language capability lists the role in `supported`.
2. The file has `files.status = indexed`.
3. No relevant `parse_diagnostics` row reports an error or missing syntax for
   the file or affected scope.
4. The artifact and capability snapshot are the intended inputs for the claim.

If capability evidence is missing or partial, absence is unknown. An unsupported
file, including a discovered source with no `files` row, is unknown. A file with
`files.status = unsupported` is unknown.

`files.status = failed_preserved` is also unknown: its prior extracted rows are
preserved after a read, parse, or extraction failure and may not describe the
current source. Relevant parse diagnostics likewise make absence unknown.

Even when every gate passes, absence means only that no matching role was
emitted for that indexed source under this extraction contract. It does not
prove that a test runner would discover no test.

## Impact And Completeness Limits

This contract does not claim semantic test-impact completeness. Extracted roles
and deterministic graph candidates can support later analysis, but they do not
prove that all behaviorally impacted tests were found. Runtime registration,
reflection, generated tests, runner configuration, and dynamic dispatch can add
tests or dependencies that are not present in the extraction artifact.

Consumers must not turn role absence, capability support, or an empty graph
candidate set into a definitive "no impacted tests" verdict.

## Ownership

- `julie-extractors` owns emitted test roles and capability/diagnostic evidence.
- Miller owns deterministic graph candidates over extracted facts.
- Eros owns runner inventory, scheduling, results, freshness, and verdicts.
