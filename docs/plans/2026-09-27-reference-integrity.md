# Reference integrity and remaining extraction gaps

## Approved outcome

The user approved reference-target correctness, incremental/fresh-scan equivalence, and revalidation of all 21 declared capability gaps. Historical exclusions are not acceptance criteria. Fix source-backed omissions; preserve uncertainty where the input does not establish a fact.

Worktree: `.worktrees/fix-reference-integrity`, branch `fix/reference-integrity`, baseline `981e16ccc0ec58f35e1c1329b7d0e836c3cc51ba`. No push or release. The active Goldfish brief records the user's direction.

## Architecture and constraints

The product remains source tree to SQLite. Existing language extractors own syntax, shared helpers own common fact semantics, and the CLI persists extraction output. Do not add workspace-global name resolution, framework discovery services, or a new comparison framework. Do not modify code-kb.

Reference fixes must prevent false concrete targets without discarding known lexical targets. Preserve unresolved call sites with their source spans, receivers, and caller context. Exercise parameter/local shadowing, sibling scopes, recursion, forward declarations, overloads and same-named methods through public extraction APIs. Respect language-specific implicit method access and the enclosing type for self receivers inside nested functions.

Incremental tests reuse `rebind_equivalence.rs` and its row-multiset comparator. Compare all fact tables and actual IDs. Exclude only its existing named metadata keys, two history tables and `files.last_revision_id/indexed_at`. JSON object-key order may normalize; arrays, roots, semantic versions and row multiplicity must match. Add, edit, delete and rename transitions must remove old facts and actually skip unchanged files. Malformed-to-valid edits must remove diagnostics; a second no-op scan must preserve facts. Windows handle lifetimes remain part of the contract.

Coverage corrections reuse existing annotations, roles and structural-fact schemas where their meaning fits. Do not classify benchmarks as tests, websocket endpoints as HTTP routes, or arbitrary React files as Next.js routes. Distinguish absent local extraction from a join requiring another file. Any retained gap needs concrete source evidence and a named closure task.

## Parallel execution contract

Commit mode: `parallel-lead-commit`. Workers neither stage nor commit and do not write memory. Lead owns shared contracts, capability manifests, goldens, docs, memory and integration.

| Task | Ownership | Dependency | Serialization required |
| --- | --- | --- | --- |
| 1. Incremental equivalence | CLI `tests/rebind_equivalence.rs`; CLI scan/writer fixes only after lead assigns a reproduced defect | Existing comparator | No |
| 2. Reference targets | Shared relationship resolution and language call emitters, with focused extractor tests | Read-only caller audit and plan review | No, relative to task 1 |
| 3. Coverage gaps | Separate language/framework modules and focused tests, assigned after gap audit | Audit all 21 declarations; lead assigns shared files | No where ownership is disjoint |
| 4. Contracts and acceptance | Registry/semantic version, capabilities, fixture sources and expected output, docs and memory | Completed source fixes | Yes; consumes tasks 1–3 |

Task 3 is split into concrete owned briefs as current source establishes the needed changes. Read-only discovery can run alongside independent implementation. Coupled shared changes run under a single owner.

## Acceptance

- [x] Reference target audit covers every caller of the shared resolver and language-specific bypasses; reproduced false bindings are fixed with public extraction checks.
- [x] Direct incremental scans after edits, deletes and renames match fresh scans in all compared tables, with unchanged files and populated optional fact tables.
- [x] All 21 declared gaps have current source-backed dispositions; all locally actionable omissions are fixed and claims have golden evidence.
- [x] Lead reviews changed code, metadata, spans and negative cases; no new global resolver or speculative framework.
- [x] Focused RED/GREEN checks pass, followed by one default suite, golden suite and strict quality report on the final source.
- [x] Windows checks cover CLI lifecycle/equivalence and affected extraction contracts.
- [x] Related worktrees are reconciled, work is committed and locally integrated, and no push or release occurs.

## Verification

All cargo and scratch commands set `CARGO_TARGET_DIR=/home/murphy/source/julie-extractors/target` and `TMPDIR=/home/murphy/source/julie-extractors/target/reference-integrity-tmp`; `/tmp` quota is exhausted. Code-kb MCP recovered after the initial indexing failure. Bounded native reads remain the fallback if the index is unavailable or stale.

Baseline shared resolution tests: 11 passed on `981e16cc`. Previous Linux default and Windows gates passed on the same implementation baseline. Workers run only affected tests. Lead owns the default tier, golden review, `node scripts/language-data-quality-report.mjs --strict`, formatting and the Windows run. Security scopes: none declared; no auth, credentials, dependency upgrade or deployment work is planned.

## Progress and gap dispositions

Read-only Astra plan review required precise lexical semantics, exact comparison exclusions, nonvacuous incremental transitions, and separate local/cross-file evidence for every gap. Those requirements are incorporated above. Minimal new vocabulary is allowed when existing facts cannot represent a benchmark or websocket endpoint honestly; no semantic overloading to avoid contract work.

Direct incremental equivalence passes all four tests in 5.09 seconds; the new sequence takes 4.46 seconds. It checks add, edit, malformed-to-valid recovery, rename, delete and two no-op scans. Every transition matches a fresh artifact; unchanged files are skipped, old paths disappear and diagnostic rows clear. No scan/writer defect was found.

Implementation ownership is split into shared reference visibility, ECMAScript `this` metadata, Actix/reqwest, Java/Kotlin testing, Swift traits/Rust benchmarks, Phoenix/Tesla, and regex conditionals. Lead owns shared collectors, pattern registries, manifests, expected output and documentation. Their briefs live under the plan's ignored `.razorback/sdd` directory.

Regex conditionals need a grammar correction. Vendor the existing parser with its license/provenance and the smallest conditional rule patch, including generated C and node types. This is an approved local implementation choice; it requires no remote fork push and no parser generator at consumer build time.

Gap discovery and source-local implementations are complete. Source-site mount facts remain separate from route declarations, since one router can have multiple mounts. A lead probe also found missing routes on typed Actix `ServiceConfig` parameters; those declarations are now extracted. The 21 dispositions below close 10 original gaps and retain 11 context/composition requirements.

The first integrated run exposed regressions in function pointers, prototype methods, Kotlin extension receivers, Lua predeclared mutual recursion, Ruby implicit calls, Zig method callers and Go Ginkgo pending hooks. Focused checks cover their known positive targets as well as shadowing and unresolved occurrences. New recursive collectors also received the existing traversal-budget guards. Final fixture regeneration and platform gates pass after these corrections.

Final Linux verification passes: `cargo xtask test default` passes 6,804 tests in 29 seconds; `cargo xtask test golden` passes all eight checks; the strict quality report records 42 languages, zero silent cells, zero quality-bar debts and 12 explicit gaps. All 26 new fixture groups parse without diagnostics. Golden review finds no new duplicate pending rows, resolved/pending overlaps or dangling structural-fact owners. Three inherited calls now remain pending instead of relying on whole-file name uniqueness. The CLI capability contract includes the two new Rust patterns.

Native Windows `cargo xtask test default` passes on implementation commit `14f4510cdb0cb55ab5d71929c92f7d433ff5094c`: 190 seconds including the rebuild; all four incremental-equivalence tests pass in 6.56 seconds. The first attempt exhausted guest disk during linking; clearing only this package's rebuildable Cargo output freed 38.8 GiB. The retry used the same source commit. Main was fast-forwarded locally to the verified implementation; related earlier worktrees are clean and already merged. Final documentation/memory changes do not alter the verified source.

### Individual gap dispositions

Fixture names below are under `fixtures/extraction/<language>/`. Closures require the final golden and strict-quality checks, not just source changes.

| Language | Original gap | Change or remaining requirement | Evidence |
| --- | --- | --- | --- |
| Rust | `actix.scope_route_cross_file_registration` | Extract typed ServiceConfig routes; retain separate mount joins | `actix_local_bindings`, public two-mount check |
| Rust | `actix.scope_route_variable_binding` | Trace local scope bindings with lexical visibility | `actix_local_bindings` |
| Rust | `actix.resource_route_guard_forms` | Emit resource paths and source-attested method guards | `actix_resources` |
| Rust | `axum.param_flavor_under_report` | Retain version/dialect uncertainty for colon segments | `axum_parameter_context` |
| Rust | `axum.cross_file_nest_join` | Retain separate route and nest facts | Existing `axum_routes` and Axum tests |
| Rust | `rust.http_client.instance_receiver` | Extract local typed fields; retain unknown external types | `reqwest_fields`, existing HTTP client fixtures |
| Rust | `rust.benchmark_harness_roles` | Emit benchmark facts without test-case roles | `benchmark_harnesses` |
| TypeScript | `nextjs.signal_free_pages_router_files` | Retain explicit project-context requirement | `signal_free_pages`, existing Next.js fixtures |
| TSX | `nextjs.signal_free_pages_router_files` | Retain explicit project-context requirement | `signal_free_pages`, existing Next.js fixtures |
| JavaScript | `nextjs.signal_free_pages_router_files` | Retain explicit project-context requirement | `signal_free_pages`, existing Next.js fixtures |
| JSX | `nextjs.signal_free_pages_router_files` | Retain explicit project-context requirement | `signal_free_pages`, existing Next.js fixtures |
| Java | `cucumber.step_binding_test_roles` | Use existing step-definition role and glue container | `cucumber_steps` |
| PHP | `laravel.route_service_provider_prefix` | Retain separate provider mount and route facts | Existing `wave2_semantics` |
| Swift | `swift_testing.test_traits` | Emit named traits and raw arguments; raw annotations already existed | `testing_traits` |
| Kotlin | `kotlin.http_client.instance_receiver` | Local typed receivers already work; retain unknown external types | Existing `http_client` and `http_client_deferred` |
| Kotlin | `kotest.data_driven_test_roles` | Emit table-check facts; retain existing withData test role | `kotest_checks` |
| Kotlin | `kotest.property_test_roles` | Emit property-check facts inside the owning test | `kotest_checks` |
| Elixir | `phoenix.non_route_macros` | Emit native socket and channel facts | `phoenix_websocket` |
| Elixir | `phoenix.cross_file_scope_prefix` | Retain separate forward sites and target routes | Existing Phoenix route/forward fixtures |
| Elixir | `elixir.http_client.tesla_middleware_base_url` | Trace local client BaseUrl middleware | `tesla_local_clients` |
| Regex | `regex_conditional_patterns` | Patch grammar, emit conditionals and preserve capture numbering | `conditionals` |

### Consumer mount joins

A consumer can join the existing Actix mount, Axum nest, Laravel provider-prefix and Phoenix forward rows to route declarations. Preserve multiple registrations and source locations. This is the named closure task for four retained composition gaps; it belongs in the consumer and this session does not modify code-kb.

### Explicit project context

Unknown receiver types require a consumer join over published type facts. Signal-free Next.js files and ambiguous Axum colon syntax need explicit framework/dialect context if the product later chooses to interpret them. Such an extraction input must participate in freshness and dependency invalidation. No per-file collector may silently read neighboring manifests and make incremental results depend on untracked inputs. These are the named closure tasks for the seven remaining context gaps.

### Branch-reset capture groups

The conditional grammar audit also identified a separate pre-existing PCRE2 gap: `(?|...)` branch-reset groups are unsupported. They share capture numbers across alternative branches, unlike ordinary groups. A follow-up must add the grammar node, reset numbering at each branch, preserve every possible capture target and cover the result with golden fixtures. This new entry is outside the original 21 declarations; conditional support does not claim support for every PCRE2 group form.
