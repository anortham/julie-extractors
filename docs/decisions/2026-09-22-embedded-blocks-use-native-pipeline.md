# Embedded Blocks Use the Native Pipeline

Supersedes the "additive role symbols only" rule of
`0006-embedded-test-call-role-remapping.md`.

## Context

HTML inline `<script>`/`<style>` blocks and Vue `<script>`/`<style>` blocks
ran through thin host-specific paths. Those paths dropped interfaces, enums,
classes, imports, doc comments, body spans, complexity, and parse diagnostics,
and they attributed every call to the host element or component. The wave-1
language gap audit (`docs/plans/2026-09-22-language-gap-closure.md`) recorded
these losses as high gaps for HTML and Vue.

## Decision

`crates/julie-extractors/src/embedded.rs` runs the full registry pipeline of
the embedded language (JavaScript, TypeScript, TSX, JSX, CSS) on the block text
and remaps every row to host coordinates: spans, body spans, stable IDs,
parent IDs, relationship and pending endpoints, identifiers, literals, source
regions, structural facts, complexity metrics, type facts, and parse
diagnostics. The host then publishes the rows under its own language.

- The embedded pipeline owns the declarations, calls, and test roles of the
  block. The host adds only what the block cannot see: HTML element symbols
  and handler attributes, and the Vue component symbol, Options API structure,
  template bindings, and component tag references.
- Calls made inside a function keep that function as the caller. The host
  element or component is the caller only for code outside any callable.
- File-scope complexity rows of all blocks fold into one host row.
- Vue section boundaries come from the shared tag scanner, which matches
  nested `<template>` tags and accepts one-line and multi-line section tags.

## Consequences

- An improvement to the JavaScript, TypeScript, or CSS extractor reaches HTML
  and Vue without host changes, and HTML and Vue goldens move with it.
- Embedded output matches a standalone file of the same language, including
  modelling (for example, JavaScript imports are import symbols, not pending
  imports).
- Test-role vocabulary stays in the shared `test_calls` seam, as 0006 requires.

## Applies To

- `crates/julie-extractors/src/embedded.rs`
- `crates/julie-extractors/src/html/scripts.rs`, `html/handlers.rs`
- `crates/julie-extractors/src/vue/`
