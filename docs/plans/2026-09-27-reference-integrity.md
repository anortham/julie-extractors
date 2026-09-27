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

- [ ] Reference target audit covers every caller of the shared resolver and language-specific bypasses; reproduced false bindings are fixed with public extraction checks.
- [x] Direct incremental scans after edits, deletes and renames match fresh scans in all compared tables, with unchanged files and populated optional fact tables.
- [ ] All 21 declared gaps have current source-backed dispositions; all locally actionable omissions are fixed and claims have golden evidence.
- [ ] Lead reviews changed code, metadata, spans and negative cases; no new global resolver or speculative framework.
- [ ] Focused RED/GREEN checks pass, followed by one default suite, golden suite and strict quality report on the final source.
- [ ] Windows checks cover CLI lifecycle/equivalence and affected extraction contracts.
- [ ] Related worktrees are reconciled, work is committed and locally integrated, and no push or release occurs.

## Verification

All cargo and scratch commands set `CARGO_TARGET_DIR=/home/murphy/source/julie-extractors/target` and `TMPDIR=/home/murphy/source/julie-extractors/target/reference-integrity-tmp`; `/tmp` quota is exhausted. MCP indexing fails for that reason; bounded native reads are the unavailable-index fallback.

Baseline shared resolution tests: 11 passed on `981e16cc`. Previous Linux default and Windows gates passed on the same implementation baseline. Workers run only affected tests. Lead owns the default tier, golden review, `node scripts/language-data-quality-report.mjs --strict`, formatting and the Windows run. Security scopes: none declared; no auth, credentials, dependency upgrade or deployment work is planned.

## Progress and gap dispositions

Read-only Astra plan review required precise lexical semantics, exact comparison exclusions, nonvacuous incremental transitions, and separate local/cross-file evidence for every gap. Those requirements are incorporated above. Minimal new vocabulary is allowed when existing facts cannot represent a benchmark or websocket endpoint honestly; no semantic overloading to avoid contract work.

Direct incremental equivalence passes all four tests in 5.09 seconds; the new sequence takes 4.46 seconds. It checks add, edit, malformed-to-valid recovery, rename, delete and two no-op scans. Every transition matches a fresh artifact; unchanged files are skipped, old paths disappear and diagnostic rows clear. No scan/writer defect was found.

Implementation ownership is split into shared reference visibility, ECMAScript `this` metadata, Actix/reqwest, Java/Kotlin testing, Swift traits/Rust benchmarks, Phoenix/Tesla, and regex conditionals. Lead owns shared collectors, pattern registries, manifests, expected output and documentation. Their briefs live under the plan's ignored `.razorback/sdd` directory.

Regex conditionals need a grammar correction. Vendor the existing parser with its license/provenance and the smallest conditional rule patch, including generated C and node types. This is an approved local implementation choice; it requires no remote fork push and no parser generator at consumer build time.

Gap discovery is complete. Source-local omissions are assigned for implementation. Source-site mount facts remain separate from route declarations, since one router can have multiple mounts. A lead probe also found missing routes on typed Actix `ServiceConfig` parameters; that local omission is included in the Actix fix. Final dispositions for each of the 21 rows follow implementation and fixture review.
