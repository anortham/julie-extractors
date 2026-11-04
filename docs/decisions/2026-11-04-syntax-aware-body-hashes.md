# Use syntax context for body hashes

## Context

Body hashes ignore code formatting and comments while preserving literal data. The lexical normalizer treats a quoted string as one opaque token, so comments inside interpolation expressions change the hash. A bounded canonical probe also reproduced missing F# and PowerShell comment forms, nested Rust block-comment handling and Lua long-bracket comments. The separate C#/Razor index already addresses interpolation syntax, partial body ranges and nested normalization costs.

## Decision

Use one shared token/range index with language-owned syntax adapters. Build the index from the extraction tree once per immutable source and grammar coordinate space. The adapters describe actual comment ranges and the code parts of interpolated strings; literal gaps, escapes, delimiters and format text remain significant. Keep the established word/punctuation tokenization and MD5 framing outside corrected syntax.

`BaseExtractor` owns the cached context. Symbol creation and subsequent body-span overrides use the same context. Public language extraction initializes it from the supplied tree; direct symbol construction can initialize it from the declaration's root. Hash setters and manual symbols must not silently revert to lexical hashing. The source remains a public `String`; the index owns offsets and tokens, without retaining tree-sitter nodes.

Vue script contexts use the existing cached native script trees. Native hashes survive coordinate remapping, and overlay symbols query the corresponding section context. QML uses its combined tree. Capability validation caches a shared syntax index for native files. For HTML and Vue wrappers it extracts once from the registered source and caches published hashes keyed by symbol language and exact body range; extraction failures and conflicting hashes fail closed. It independently checks original source coordinates, declaration containment and mutations of language/range/hash. Hash-invariance and exact-span tests provide separate semantic checks. Cache strategy follows the host source identity, never the first emitted symbol's language.

## Rejected alternatives

- Parsing or copying a whole file for each symbol repeats work as output grows.
- A canonical-only rehash leaves public native extractors inconsistent.
- Separate textual interpolation parsers duplicate grammar decisions.
- Treating every node with a comment-like name as code loses Swift literal data.
- Normalizing literal whitespace or format text changes the contract's meaning.

## Verification

Current-source regressions must reproduce the probe failures before implementation. Required checks cover comment insertion/removal/length changes, interpolation code formatting and token changes, literal and format-text sensitivity, nested interpolation/comments/strings, partial lambda bodies, Unicode/CRLF and unchanged spans/owners. Unaffected controls retain their exact prior hashes. Flat and nested work counters at N/2N/4N must stay within a 2.5 doubling bound, including index construction and small range queries. Final corpus, compatibility, performance and Windows gates remain part of the language-coverage goal.

## Consequences

Corrected bodies receive new hashes and require fresh extraction under a declared output marker. SQLite schema and symbol identity do not change. Future hash corrections belong in the shared engine or the relevant syntax adapter, with public extraction regressions and bounded work evidence.
