# Syntactic variable access facts

Variable access lives in the optional `identifiers.metadata_json.access` key.
It describes syntax, with `write`, `read_write`, and `read`; a missing key
means unclassified. It does not require binding resolution or infer what a
callee does to an argument.

A shared classifier walks identifier ancestors using language-specific tables
of node kinds, child selectors, operators, and context guards. Identifier
creation attaches the result before embedded coordinates are mapped. Receivers
and subscript indices remain reads when a member or indexed location is
written. Explicit reference markers classify only the syntax they mark.

Extractors add missing assignment and pattern references through that shared
classifier. Declarations already represented by symbols do not gain duplicate
references. In languages whose extractor emits a symbol for every assignment,
an earlier same-name binding under the same owner distinguishes reassignment
from the first binding. This check is local extraction evidence; it does not
resolve references across files.

New fallback references retain the existing owner of an exact token span:
a member or call row prevents a second variable row at that token. This guard
applies at emission, preserving the original identifier kinds and other facts.
Discard placeholders do not become variable references.

`kind_coverage.identifiers` retains its closed identifier-kind vocabulary.
The additive `kind_coverage.identifier_metadata` section records
`identifiers.access` support, backed by registered golden fixtures. Native
schema links and aliases remain unclassified when they have no variable-access
semantics. Known XML build-property substitutions are reads; embedded code
uses its guest language's rules.

The optional key and capability section keep SQLite schema v8 and extraction
contract v4. The extraction fingerprint changes through the
`variable-write-sites-v1` marker, requiring a forced refresh of older artifacts.
