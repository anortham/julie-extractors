# Canonical receiver facts

Receiver names and qualification belong to extraction. The CLI must serialize
them without interpreting source syntax.

The previous CLI byte scanner accepted ASCII names and a short list of member
separators. It missed valid syntax and could turn CSS selectors or fragments of
expression receivers into object references. Rust API callers also received
different facts from CLI callers.

Use the syntax tree at the shared registry boundary to enrich identifiers.
Preserve language-owned receiver metadata as an authoritative pair and keep
self-type evidence independent. Infer named chains only; expression receivers
need richer evidence than the existing name fields can express.

Embedded extraction runs this pass before remapping coordinates. Skip Vue's
outer pass because its scripts and template expressions already pass through
the registry with their own trees before Vue relabels the results.

SQL call producers publish their existing parsed qualification parts, including
quoted names. Rust macro extraction reuses item-macro trees and parses expression
macro bodies with their original byte positions preserved. Nested expression
macros require another parse per nesting level, bounded by the existing extraction
depth limit. Revisit direct token-tree handling if profiling shows this work is costly.

No schema change or resolver is needed. The semantic extraction version changes,
so existing artifacts require a forced scan. Reference spans and IDs remain
unchanged by metadata enrichment.

The conformance work also corrects broad target spans in R and C++ qualified
calls, HTML ID/class values, regex named backreferences, and C# target-typed
construction. C# keeps the inferred type name and anchors the call on the written
`new` keyword. C++ and Rust emit one call identifier per member callee. R member lookup
uses grammar fields so intervening comments cannot become targets.
Ruby setter declaration names no longer produce false call references.

The public API and SQLite contract matrix account for every registered
language. Golden output records the resulting receiver metadata.
