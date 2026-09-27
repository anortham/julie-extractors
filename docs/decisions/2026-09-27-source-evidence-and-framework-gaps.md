# Source evidence and framework gaps

The 2026-09-27 correctness review rechecks current code and fixtures. Earlier exclusions do not constrain the implementation.

Local call targets require lexical visibility and shadowing evidence before uniqueness. A single matching name somewhere in a file is insufficient. Unknown targets retain their occurrence, receiver and caller facts for downstream resolution. Language lookup rules remain explicit; extraction does not gain a workspace resolver.

Framework facts describe written declarations. A route definition and a mount remain separate because the same router can be mounted at multiple prefixes. Missing local route declarations, such as Actix `ServiceConfig` calls, are extraction defects. Computing a deployment's complete route graph is a join over those facts.

Testing constructs retain their actual meaning. Cucumber glue methods use the existing `step_definition` role. Kotest table/property checks describe work inside a test and do not create extra test cases. Swift traits publish their written names and arguments without evaluating conditions. Rust benchmark registrations use structural testing facts rather than `test_case`.

Phoenix socket/channel declarations use the `websocket` family rather than HTTP route facts. Existing metadata, span and ownership contracts apply to these new patterns. No new SQLite table is required.

The current regex grammar misparses conditionals and can corrupt capture numbering. A small licensed local grammar patch preserves one syntax tree as the authority. Generated parser files ship in the repository so consumers do not need a parser generator. The package version is `0.25.0-julie.1`, and its capability row uses the new `vendored` dependency status. No remote fork push is part of this work.

Project context remains explicit uncertainty. A signal-free React component under `pages/` does not establish Next.js ownership. An unknown client's type does not establish an HTTP request. Axum colon segments can be captures in older versions or literals with current `without_v07_checks`; the raw template is preserved without claiming parameter semantics the input does not prove.

Evidence and acceptance are tracked in [the implementation plan](../plans/2026-09-27-reference-integrity.md). The framework semantics were checked against [Kotest table testing](https://kotest.io/docs/framework/datatesting/data_driven_testing_4.2.0/), [Kotest property tests](https://kotest.io/docs/proptest/property-test-functions.html), [Swift Testing traits](https://docs.swift.org/latest/documentation/testing/traits/), [Tesla BaseUrl policies](https://tesla.hexdocs.pm/Tesla.Middleware.BaseUrl.html), [Axum path validation](https://docs.rs/axum/latest/src/axum/routing/path_router.rs.html) and [PCRE2 conditional patterns](https://pcre.org/current/doc/html/pcre2pattern.html).
