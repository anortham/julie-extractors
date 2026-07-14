# Parser Runtime and Grammar Freshness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Update the Tree-sitter runtime and the C#, Swift, and R grammars with parser-backed extraction evidence, then add a repeatable report for finding future runtime and grammar drift.

**Architecture:** Keep parser migration logic inside the affected language modules, pin the C# grammar to one exact pushed commit in an owned fork, and preserve all existing artifact interfaces. Add one networked maintenance script whose registry and GitHub adapters feed pure, deterministic comparison and rendering functions; it is never part of the default test tier.

**Tech Stack:** Rust 2024, Tree-sitter runtime and CLI 0.26.11, Cargo, Node.js ESM, crates.io API, GitHub API/CLI, JSON extraction fixtures, and xtask test tiers.

**Architecture Quality:** The only new caller-facing interface is `node scripts/grammar-freshness-report.mjs [--format text|json]`; `julie-extract`, SQLite, JSONL, extraction facts, and schema contracts remain unchanged. Grammar-node adaptations remain language-local and remote metadata logic remains in the maintenance script. Architecture risk is medium because runtime and grammar changes can silently change node shapes across many languages.

## Global Constraints

- Work only in `/Users/murphy/source/julie-extractors/.worktrees/tsql-parse-quality-current` on `codex/parser-grammar-freshness`, except Task 2's isolated `tree-sitter-c-sharp` fork worktree.
- The integration base is the completed T-SQL commit `dbff11b8598e47eea867c1cc69484561b9877b3e`; its SQL and Razor behavior is a hard regression contract.
- Preserve the untracked plan at `/Users/murphy/source/julie-extractors/docs/plans/2026-07-11-tsql-parse-quality-implementation-plan.md` without modification.
- Declare and lock the Rust Tree-sitter runtime at exactly `0.26.11`; use Tree-sitter CLI `0.26.11` for newly generated parser artifacts.
- Create `anortham/tree-sitter-c-sharp` from upstream `tree-sitter/tree-sitter-c-sharp@af29416d729b7a6603101b513604392d8f675e3b`, push the accepted grammar commit, and pin Julie Extractors to that exact remote commit.
- Do not depend on local grammar paths, floating Git refs, unpushed commits, or unreleased upstream C# state.
- Preserve existing general-language node interpretation, extraction behavior, registered goldens, capability claims, structural-fact registry, artifact schema, CLI behavior, SQLite, and JSONL contracts except for reviewed evidence-backed additions.
- Valid C#, Swift, R, SQL, and Razor fixtures must emit zero `error` and `missing` diagnostics; malformed controls must continue to emit diagnostics.
- Keep corpus scans, network freshness checks, complete parser certification, and other slow gates outside the default test tier.
- Do not edit `/Users/murphy/source/julie`, `/Users/murphy/source/tree-sitter-razor`, `/Users/murphy/source/miller`, or `/Users/murphy/source/eros`.
- The approved C# grammar fork may be created and pushed. Do not push Julie Extractors, open an upstream C# pull request, tag, publish, release, or choose a Julie Extractors version without separate explicit approval.
- Follow TDD: establish a failing parser, extraction, or contract test before implementation; generated Cargo and parser artifacts are updated only after the source test or contract is red.
- On any unexpected failure, use `razorback:systematic-debugging` before changing implementation.

## Current Evidence

- Julie Extractors currently declares `tree-sitter = "0.26.8"` and locks `0.26.9`; Tree-sitter runtime and CLI `0.26.11` are current.
- `tree-sitter-c-sharp 0.23.5` is the newest published crate, but upstream C# 14 support landed later at `af29416d729b7a6603101b513604392d8f675e3b` and excludes .NET file-app directives.
- Microsoft documents C# 14 for .NET 10 and the `#:include`, `#:package`, `#:project`, `#:property`, and `#:sdk` file-app directives plus shebang handling.
- `tree-sitter-swift 0.7.3` supersedes the locked `0.7.2`; `tree-sitter-r 1.3.0` supersedes the locked `1.2.0`.
- The pinned Razor grammar is one documentation-only commit behind its fork head; PowerShell is three folds/tests-only commits behind; QML is eleven packaging/reference-only commits behind; Visual Basic and SQL match their fork heads. These pins do not change in this plan.
- The integration base fails `dependency_policy_allows_pinned_git_parser_sources` and the Cargo-deny source check because `deny.toml` omits `https://github.com/anortham/tree-sitter-sql`; Task 1 repairs this inherited T-SQL policy gap before establishing its new runtime RED.

## File and Interface Map

- `crates/julie-extractors/Cargo.toml` and `Cargo.lock`: exact runtime and grammar dependency resolution.
- `deny.toml` and `xtask/tests/release_contract.rs`: owned, exact, approved Git parser-source policy.
- `crates/julie-extractors/src/{csharp,swift,r}/`: language-local node interpretation; change only when a red migration test proves it is necessary.
- `crates/julie-extractors/src/tests/{csharp,swift,r}/`: focused parser-diagnostic and extraction behavior tests.
- `fixtures/extraction/{csharp,swift,r}/` and `fixtures/extraction/capabilities.json`: canonical evidence for supported language behavior.
- `scripts/grammar-freshness-report.mjs`: networked maintenance CLI and pure comparison/rendering functions.
- `scripts/grammar-freshness-report.test.mjs`: network-free report contract tests.
- `docs/architecture/grammar-dependency-policy.md`: runtime, generation, registry, Git-pin, ownership, and freshness policy.
- `/Users/murphy/source/tree-sitter-c-sharp`: does not exist yet; Task 2 creates it by cloning upstream `tree-sitter/tree-sitter-c-sharp` and checking out `af29416d729b7a6603101b513604392d8f675e3b`.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `CLAUDE.md`, `xtask/src/test_tiers.rs`, `crates/julie-extractors/src/tests/test_tiers.rs`, `docs/plans/2026-07-14-parser-runtime-and-grammar-freshness-design.md`, and the dependency policy produced by Task 1.

**Worker red/green scope:** Use the narrowest focused Rust test filter, Node test, or grammar corpus test that proves the assigned behavior. Every behavioral task must retain evidence of the pre-implementation failure and the post-implementation pass.

**Worker ceiling:** Workers may run their focused test, the affected `cargo xtask test language <name>` tier, and directly related golden, capability, or dependency-policy tests. Workers do not own the default, all-language, certification, Terraform replay, Clippy, or full branch gates.

**Worker gate invariant:** Runtime tests prove exact declared/locked resolution and Git-source policy; C#/Swift/R language gates prove valid syntax has zero diagnostics and useful stable extraction while malformed controls remain diagnostic; report tests prove deterministic versioned output and error behavior without network access.

**Lead affected-change scope:** After each accepted language migration, run its language tier plus golden and capability tiers. After manifest changes, run dependency-policy tests, `cargo tree --locked`, and `cargo deny --all-features check`. After the final coherent batch, run `cargo xtask test changed` against all changed Julie paths if the command accepts the resulting path set.

**Branch gate:** Run focused C#/Swift/R tests; `cargo xtask test language csharp`, `swift`, and `r`; golden, capability, contract, certification, and default tiers; `cargo fmt --all -- --check`; workspace Clippy with warnings denied; Cargo deny; strict language quality; report unit tests; a live JSON freshness report; a fresh CLI build; exact Cargo resolution checks; and the pinned Terraform SQL/Razor replay.

**Replay/metric evidence:** Hard gates are `silent_cells=0`, `quality_bar_debts=0`, zero valid SQL diagnostics, zero valid Razor diagnostics, nonzero malformed T-SQL diagnostics, zero valid C#/Swift/R fixture diagnostics, and exact runtime/C# fork resolution. Freshness rows for dependencies intentionally left unchanged are report-only and must match the audited reasons in this plan.

**Escalation triggers:** Any shared-runtime regression, existing golden drift, public artifact-shape change, structural-fact registry change, diagnostic regression outside the targeted fixture, multiple runtime versions, or unexpected Git resolution requires the full certification and default tiers before proceeding. Any grammar generation or corpus failure requires full grammar corpus and binding tests in that grammar repository.

**Assigned verification failure:** Workers use systematic debugging for failures inside their owned implementation. They stop and report a plan mismatch, ownership conflict, external access failure, or failing unrelated gate rather than weakening tests or expanding product intent.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in `.razorback/sdd/verification-ledger.md`. For the Terraform replay and quality report, also record the hard-gate counts. Reuse an exact-HEAD passing entry for an unchanged expensive scope.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Lock the runtime and dependency policy | Batch A | `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `deny.toml`, `xtask/tests/release_contract.rs`, `docs/architecture/grammar-dependency-policy.md` | No | None - safe parallel batch; Task 2 uses a separate repository and Git index. |
| Task 2: Create and push the owned C# grammar fork | Batch A | `/Users/murphy/source/tree-sitter-c-sharp/**` and the remote `anortham/tree-sitter-c-sharp` fork | No | None - safe parallel batch; Task 1 uses only the Julie worktree. |
| Task 3: Integrate C# 14 extraction evidence | None - serial | `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `deny.toml`, `xtask/tests/release_contract.rs`, `crates/julie-extractors/src/csharp/**`, `crates/julie-extractors/src/tests/csharp/**`, `fixtures/extraction/csharp/csharp14/**`, `fixtures/extraction/capabilities.json` | Yes | Requires the exact pushed Task 2 parser commit and the Task 1 runtime/policy baseline. |
| Task 4: Migrate Swift to 0.7.3 | None - serial | `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/swift/**`, `crates/julie-extractors/src/tests/swift/**`, `fixtures/extraction/swift/current_syntax/**`, `fixtures/extraction/capabilities.json` | Yes | Serializes shared manifest, lockfile, capability registry, and golden review after C#. |
| Task 5: Migrate R to 1.3.0 | None - serial | `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/r/**`, `crates/julie-extractors/src/tests/r/**`, `fixtures/extraction/r/current_syntax/**`, `fixtures/extraction/capabilities.json` | Yes | Serializes shared manifest, lockfile, capability registry, and golden review after Swift. |
| Task 6: Add the grammar freshness report | None - serial | `scripts/grammar-freshness-report.mjs`, `scripts/grammar-freshness-report.test.mjs`, `docs/architecture/grammar-dependency-policy.md`, `crates/julie-extractors/src/tests/test_tiers.rs`, `xtask/src/test_tiers.rs`, `xtask/tests/test_tiers.rs` | Yes | Requires the final manifest and lockfile so fixtures and live output represent the accepted dependency state. |
| Task 7: Certify the integrated branch | None - serial | `.razorback/sdd/verification-ledger.md` and plan acceptance checkboxes only | Yes | Requires Tasks 1-6 complete at one stable Julie commit and the pushed C# fork commit. |

Task 1 and Task 2 use `serial-worker-commit` in their separate repositories. Tasks 3-6 also use `serial-worker-commit` because they execute sequentially in the Julie worktree. The lead reviews each owned diff and verification evidence before dispatching its dependent task. Task 7 is lead-owned and does not create a product-code commit unless certification reveals a defect.

### Task 1: Lock the Tree-sitter runtime and dependency policy

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `deny.toml`
- Modify: `xtask/tests/release_contract.rs`
- Create: `docs/architecture/grammar-dependency-policy.md`

**Interfaces:**
- Consumes: Cargo dependency resolution and the existing `dependency_policy_allows_pinned_git_parser_sources` release contract.
- Produces: one exact Rust runtime resolution at `0.26.11` and a documented policy requiring exact, pushed, approved parser sources and Tree-sitter CLI `0.26.11` for future generation.

**Contract inputs:** Tree-sitter runtime/CLI `0.26.11`; existing exact Git pins; no historical generated-parser record is rewritten.

**File ownership:** `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `deny.toml`, `xtask/tests/release_contract.rs`, `docs/architecture/grammar-dependency-policy.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch; Task 2 uses a separate repository and Git index.

**Approach:** First reproduce and repair the inherited SQL `allow-git` failure by adding `https://github.com/anortham/tree-sitter-sql` to `deny.toml`. Then strengthen release-contract tests so the current `0.26.8` declaration/`0.26.9` lock and any non-exact Git parser source fail. Declare `tree-sitter = "=0.26.11"`, resolve the lockfile deliberately, and document registry-vs-Git selection, owned-fork requirements, generation version, audit cadence, semantic-evidence requirements, and the boundary between drift detection and support claims. Do not update any grammar in this task.

**Acceptance:**
- [x] The release contract is observed failing on the old runtime declaration or lock.
- [x] The inherited SQL `allow-git` failure is recorded separately and repaired before the runtime RED is established.
- [x] Manifest and lockfile resolve exactly one `tree-sitter 0.26.11` runtime.
- [x] Every Git parser dependency has an exact `rev`, an allowed remote source, and a matching locked commit.
- [x] The architecture policy records Tree-sitter CLI `0.26.11` as the future generation floor and preserves historical records.
- [x] Focused release-contract tests, `cargo tree --locked`, and `cargo deny --all-features check` pass.
- [x] The worker commits the verified Julie-owned files and reports path, branch, commit, and dirty state.

### Task 2: Create and push the owned C# grammar fork

**Files:**
- Create or modify in `/Users/murphy/source/tree-sitter-c-sharp`: `test/corpus/preprocessor-directives.txt` or the existing upstream preprocessor corpus file discovered before editing
- Modify in `/Users/murphy/source/tree-sitter-c-sharp`: `grammar.js`
- Regenerate in `/Users/murphy/source/tree-sitter-c-sharp`: `src/grammar.json`, `src/node-types.json`, `src/parser.c`
- Modify only if generated/binding tests require it: grammar package metadata and binding lockfiles

**Interfaces:**
- Consumes: upstream `tree-sitter/tree-sitter-c-sharp@af29416d729b7a6603101b513604392d8f675e3b` and Tree-sitter CLI `0.26.11`.
- Produces: a pushed `anortham/tree-sitter-c-sharp` commit whose grammar recognizes complete shebang and .NET file-app directive lines, exposed to Task 3 as `C_SHARP_FORK_COMMIT`.

**Contract inputs:** Supported line forms are `#!...` and `#:include`, `#:package`, `#:project`, `#:property`, and `#:sdk`; the grammar accepts the complete directive line but does not interpret SDK/MSBuild semantics. Existing preprocessor syntax and malformed-code diagnostics remain intact.

**File ownership:** `/Users/murphy/source/tree-sitter-c-sharp/**` and the remote `anortham/tree-sitter-c-sharp` fork

**Serialization required:** No

**Dependency reason:** None - safe parallel batch; Task 1 uses only the Julie worktree.

**Approach:** Clone upstream `tree-sitter/tree-sitter-c-sharp` to `/Users/murphy/source/tree-sitter-c-sharp` because the path does not exist yet, check out the exact upstream base commit, then audit remotes, default branch, generated-file commands, and dirty state. Create an isolated fork worktree from the exact upstream commit. Add corpus cases first and record that upstream emits errors for valid file-app lines. Introduce one narrow named node for complete file-app directive lines, regenerate with CLI `0.26.11`, and review every generated node-shape change. Add malformed controls that continue to produce `ERROR` or `MISSING`. Create the owned GitHub fork if absent, commit, push the accepted commit to the fork's default branch, and verify the remote object before reporting it. Do not open an upstream pull request.

**Acceptance:**
- [x] Upstream failure evidence is recorded before the grammar change.
- [x] Corpus coverage includes shebang and every supported `#:` directive, including values with spaces, quoted paths/packages, and CRLF boundaries.
- [x] Malformed directive/preprocessor controls remain diagnostic.
- [x] Grammar generation uses CLI `0.26.11`; generated sources match `grammar.js`.
- [x] Full grammar corpus, highlight/query checks present in the repo, Rust tests/doctests, and available binding tests pass.
- [x] `anortham/tree-sitter-c-sharp` contains the accepted commit on its remote default branch.
- [x] The worker reports `C_SHARP_FORK_COMMIT`, upstream base, commands/results, path, branch, commit, remotes, and dirty state.

### Task 3: Integrate C# 14 and file-app extraction evidence

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `deny.toml`
- Modify: `xtask/tests/release_contract.rs`
- Modify: `crates/julie-extractors/src/tests/csharp/mod.rs`
- Create: `crates/julie-extractors/src/tests/csharp/csharp14.rs`
- Modify only when a failing migration test proves it: `crates/julie-extractors/src/csharp/**`
- Create: `fixtures/extraction/csharp/csharp14/source.cs`
- Create: `fixtures/extraction/csharp/csharp14/expected.json`
- Modify: `fixtures/extraction/capabilities.json`

**Interfaces:**
- Consumes: `C_SHARP_FORK_COMMIT`, runtime `0.26.11`, canonical extraction helpers, parser diagnostic helpers, golden fixture registry conventions, and dependency-source policy.
- Produces: an exact remote C# grammar pin and registered `csharp:csharp14` evidence with stable existing artifact rows and zero valid diagnostics.

**Contract inputs:** Official C# 14 syntax and .NET file-app lines; existing artifact pattern names and schemas; no new pattern or schema version unless parser-backed evidence proves an existing contract cannot represent a required fact.

**File ownership:** `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `deny.toml`, `xtask/tests/release_contract.rs`, `crates/julie-extractors/src/csharp/**`, `crates/julie-extractors/src/tests/csharp/**`, `fixtures/extraction/csharp/csharp14/**`, `fixtures/extraction/capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Requires the exact pushed Task 2 parser commit and the Task 1 runtime/policy baseline.

**Approach:** Add failing diagnostic/extraction tests and the canonical fixture before changing the dependency. Cover extension declarations, null-conditional assignment, unbound generic `nameof`, field-backed properties, simple-lambda parameter modifiers, partial constructors/events, user-defined compound assignment, shebang, and all supported `#:` directives. Pin the owned fork by exact `rev`, allow that exact remote in dependency policy, and update only C#-local matchers proven incompatible by the tests. Generate the expected golden from the canonical extractor, inspect it row by row, and register only evidence-backed capability changes. Keep malformed C# and directive controls diagnostic.

**Acceptance:**
- [x] The published `0.23.5` parser is observed failing the valid C# 14/file-app case before the pin changes.
- [x] Cargo metadata and lockfile resolve C# from `https://github.com/anortham/tree-sitter-c-sharp` at exactly `C_SHARP_FORK_COMMIT`.
- [x] The complete valid fixture has zero `error` and `missing` diagnostics.
- [x] Each official feature produces the expected existing symbol, relationship, identifier, type, literal, or structural rows where semantically applicable.
- [x] Malformed C# and file-app controls emit diagnostics.
- [x] Existing C# goldens and public artifact shapes remain stable.
- [x] Focused tests, `cargo xtask test language csharp`, golden, capability, and release dependency-policy tests pass.
- [x] The worker commits and reports path, branch, commit, and dirty state.

### Task 4: Migrate Swift to 0.7.3

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/julie-extractors/src/tests/swift/mod.rs`
- Create: `crates/julie-extractors/src/tests/swift/current_syntax.rs`
- Modify only when a failing migration test proves it: `crates/julie-extractors/src/swift/**`
- Create: `fixtures/extraction/swift/current_syntax/source.swift`
- Create: `fixtures/extraction/swift/current_syntax/expected.json`
- Modify: `fixtures/extraction/capabilities.json`

**Interfaces:**
- Consumes: canonical Swift extraction, diagnostic helpers, golden registry conventions, and `tree-sitter-swift 0.7.3`.
- Produces: a registry-backed Swift `0.7.3` resolution and current-syntax evidence with stable public artifacts.

**Contract inputs:** Cases cover consume/discard operators, typed-throws do/catch, parenthesized `nonisolated`, conditional directives inside type bodies, bracket-qualified nested types, and double-optional lambda parameter types.

**File ownership:** `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/swift/**`, `crates/julie-extractors/src/tests/swift/**`, `fixtures/extraction/swift/current_syntax/**`, `fixtures/extraction/capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Serializes shared manifest, lockfile, capability registry, and golden review after C#.

**Approach:** Freeze current 0.7.2 behavior with failing parser/extraction tests, bump only Swift to `=0.7.3`, inspect upstream node inventory changes, and adapt language-local extraction only for proven node-shape migrations. Generate and manually review the registered golden; do not accept unrelated row drift. Include malformed controls for the new constructs.

**Acceptance:**
- [x] At least one targeted current-syntax case is observed failing or producing an extraction gap under 0.7.2.
- [x] Cargo resolves exactly `tree-sitter-swift 0.7.3`.
- [x] The valid fixture has zero `error` and `missing` diagnostics and useful stable extraction.
- [x] Malformed current-syntax controls remain diagnostic.
- [x] Existing Swift goldens and artifact shapes remain stable.
- [x] Focused tests, `cargo xtask test language swift`, golden, and capability tiers pass.
- [x] The worker commits and reports path, branch, commit, and dirty state.

### Task 5: Migrate R to 1.3.0

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/julie-extractors/src/tests/r/mod.rs`
- Create: `crates/julie-extractors/src/tests/r/current_syntax.rs`
- Modify only when a failing migration test proves it: `crates/julie-extractors/src/r/**`
- Create: `fixtures/extraction/r/current_syntax/source.R`
- Create: `fixtures/extraction/r/current_syntax/expected.json`
- Modify: `fixtures/extraction/capabilities.json`

**Interfaces:**
- Consumes: canonical R extraction, diagnostic helpers, golden registry conventions, and `tree-sitter-r 1.3.0`.
- Produces: a registry-backed R `1.3.0` resolution and current-syntax evidence while preserving public artifact shapes.

**Contract inputs:** Cases cover `return` as an ordinary identifier, hexadecimal constants with decimals, identifiers beginning with `else`, raw-string open/content/close nodes, and CRLF comment boundaries.

**File ownership:** `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/r/**`, `crates/julie-extractors/src/tests/r/**`, `fixtures/extraction/r/current_syntax/**`, `fixtures/extraction/capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Serializes shared manifest, lockfile, capability registry, and golden review after Swift.

**Approach:** Add failing parser/extraction tests against 1.2.0, bump only R to `=1.3.0`, inspect the new raw-string and token nodes, and adapt R-local extractors only where canonical facts would be lost or reclassified. Generate and review the registered golden and keep malformed controls diagnostic.

**Acceptance:**
- [ ] At least one targeted syntax or node-shape case is observed failing under 1.2.0.
- [ ] Cargo resolves exactly `tree-sitter-r 1.3.0`.
- [ ] The valid fixture has zero `error` and `missing` diagnostics and useful stable extraction.
- [ ] Malformed current-syntax controls remain diagnostic.
- [ ] Existing R goldens and artifact shapes remain stable.
- [ ] Focused tests, `cargo xtask test language r`, golden, and capability tiers pass.
- [ ] The worker commits and reports path, branch, commit, and dirty state.

### Task 6: Add a deterministic grammar freshness report

**Files:**
- Create: `scripts/grammar-freshness-report.mjs`
- Create: `scripts/grammar-freshness-report.test.mjs`
- Modify: `docs/architecture/grammar-dependency-policy.md`
- Modify only if needed to lock the convention: `crates/julie-extractors/src/tests/test_tiers.rs`
- Modify only if needed to lock an explicit maintenance tier: `xtask/src/test_tiers.rs`
- Test only if xtask changes: `xtask/tests/test_tiers.rs`

**Interfaces:**
- Consumes: `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, crates.io package metadata, and GitHub repository/default-branch metadata.
- Produces: `node scripts/grammar-freshness-report.mjs [--format text|json]` with deterministic text or a JSON object containing `schema_version: 1`, audit metadata, and separately ordered `runtime`, `registry_grammars`, and `git_grammars` rows.

**Contract inputs:** Declared requirements and exact locked versions/commits are distinct fields. Registry comparison uses published stable versions. Git comparison uses the pinned commit and current remote default head. Network/source failures identify the source and return nonzero. Drift is reported without claiming semantic support.

**File ownership:** `scripts/grammar-freshness-report.mjs`, `scripts/grammar-freshness-report.test.mjs`, `docs/architecture/grammar-dependency-policy.md`, `crates/julie-extractors/src/tests/test_tiers.rs`, `xtask/src/test_tiers.rs`, `xtask/tests/test_tiers.rs`

**Serialization required:** Yes

**Dependency reason:** Requires the final manifest and lockfile so fixtures and live output represent the accepted dependency state.

**Approach:** Export pure parsing, normalization, comparison, ordering, and rendering functions without triggering the CLI on import. Write network-free Node tests first for manifest/lock parsing, prerelease ordering, Git URL normalization, deterministic row ordering, schema output, text output, invalid arguments, and adapter failure mapping. Add thin crates.io and GitHub adapters with explicit timeouts and source-labelled errors. Invoke network work only from the executable entry point. Document the command and audit interpretation. Because the default xtask plan runs only Rust crate tests, change test-tier code only if an enforceable convention is missing; do not add the live report to default, certification, or changed tiers.

**Acceptance:**
- [ ] Node tests are observed failing before the report implementation.
- [ ] `node --test scripts/grammar-freshness-report.test.mjs` passes without network access.
- [ ] Repeated fixture-backed JSON/text renders are byte-for-byte deterministic apart from an explicit audit timestamp, which tests inject.
- [ ] JSON output has `schema_version: 1` and stable, documented row fields and ordering.
- [ ] Unsupported flags, crates.io failures, GitHub failures, and malformed metadata return nonzero with the failed source identified.
- [ ] A live `--format json` run reports exact accepted runtime and grammar resolution and reports unchanged-pin drift separately.
- [ ] Default tier planning contains no network command and remains within its 90-second budget contract.
- [ ] The worker commits and reports path, branch, commit, and dirty state.

### Task 7: Certify the integrated branch

**Files:**
- Modify: `.razorback/sdd/verification-ledger.md`
- Modify after every gate passes: `docs/plans/2026-07-14-parser-runtime-and-grammar-freshness-implementation-plan.md`

**Interfaces:**
- Consumes: accepted commits from Tasks 1-6, `C_SHARP_FORK_COMMIT`, Terraform commit `821e6b1a268cb392b1abb5080243a299db2a9bc9`, and the repository's existing test-tier/CLI interfaces.
- Produces: exact, timestamped release-readiness evidence at one clean Julie commit and one pushed C# parser commit.

**Contract inputs:** Hard gates from the Verification Strategy; the completed T-SQL scan commands and malformed controls from the T-SQL implementation plan; no Julie push or release operation.

**File ownership:** `.razorback/sdd/verification-ledger.md` and plan acceptance checkboxes only

**Serialization required:** Yes

**Dependency reason:** Requires Tasks 1-6 complete at one stable Julie commit and the pushed C# fork commit.

**Approach:** Re-read the original request, design, this plan, and accepted diffs. Run format and static analysis first, then focused/language/golden/capability/contract/certification/default tiers, strict quality, report tests/live report, fresh CLI build, exact Cargo resolution, and the Terraform valid/malformed replay. Use the fresh CLI binary for corpus evidence. If a gate fails, identify the root cause, fix it with a new focused red/green test, re-run the invalidated scopes, and continue. Request an inline code review after the branch gate, verify findings against live code, fix accepted findings, and repeat invalidated gates. Record final worktree state for every related Julie and grammar worktree.

**Acceptance:**
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings` passes.
- [ ] Focused C#, Swift, and R parser/extractor tests pass.
- [ ] `cargo xtask test language csharp`, `cargo xtask test language swift`, and `cargo xtask test language r` pass.
- [ ] `cargo xtask test golden`, `capability`, `contract`, `certification`, and `default` pass.
- [ ] `cargo deny --all-features check` passes.
- [ ] `node scripts/language-data-quality-report.mjs --strict` reports `silent_cells=0` and `quality_bar_debts=0`.
- [ ] `node --test scripts/grammar-freshness-report.test.mjs` and a live `node scripts/grammar-freshness-report.mjs --format json` pass.
- [ ] A fresh `julie-extract` build is used for the Terraform replay at `821e6b1a268cb392b1abb5080243a299db2a9bc9`.
- [ ] All six valid Terraform SQL files emit zero SQL `error` and `missing` diagnostics.
- [ ] Valid Terraform Razor files emit zero Razor `error` and `missing` diagnostics.
- [ ] Malformed T-SQL negative controls still emit diagnostics.
- [ ] `cargo metadata --locked` and `cargo tree --locked` prove runtime `0.26.11`, Swift `0.7.3`, R `1.3.0`, and exact remote `C_SHARP_FORK_COMMIT` resolution without local paths.
- [ ] The C# parser commit exists on `anortham/tree-sitter-c-sharp`'s remote default branch.
- [ ] Review has no unresolved findings and all invalidated verification has been repeated.
- [ ] The Julie worktree is clean and release-ready; all related Julie and grammar worktrees have reported path, branch, commit, and dirty state.
- [ ] Julie remains unpushed, untagged, unpublished, unversioned, and unreleased pending explicit approval.

## Final Handoff

Report the exact Julie Extractors commit, exact pushed C# parser commit, old/new runtime and grammar resolution, C#/Swift/R/SQL/Razor valid and malformed diagnostic counts, every verification command/result, every related worktree state, any report-only freshness rows, and the remaining Julie push/version/release approval boundary.
