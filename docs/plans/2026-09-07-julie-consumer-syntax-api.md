# Julie Consumer Syntax API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Give Julie a supported Rust syntax interface and nameable relationship fact types so it can adopt the current extractor release without restoring removed internals or weakening its editing safeguards.

**Architecture:** Add an optional `syntax-api` Cargo feature exposing one source-to-host-tree operation, with the existing normalized diagnostic type. Share grammar selection with canonical extraction while retaining its existing public behavior. Source ownership, editing decisions, search projections, and workspace resolution remain consumer responsibilities.

**Tech Stack:** Rust 2024, the existing pinned `tree-sitter = "=0.26.11"`, `anyhow`, and Cargo's external integration-test targets. No new dependency.

**Architecture Quality:** The approved public boundary is `syntax::{parse_source, parse_source_with_options, SyntaxOptions, ParsedSource, SyntaxError}`, plus root exports for existing relationship value types. Internal modules stay private. Risk is medium: header detection currently hides probe failures, diagnostics have a traversal ceiling, and composite files do not have a single complete embedded-language AST. The plan addresses these separately without changing extraction output. Report a contradiction of this boundary to the lead; do not expose grammar loaders or implement editing tools upstream.

## Global Constraints

- This is a full implementation handoff, not evidence that implementation or verification has happened. All snippets marked proposed describe new code.
- Work in `/home/murphy/source/julie-extractors`, the checkout explicitly selected by the user. Inspect current branch/worktrees/dirty state before execution; do not overwrite unrelated work or create another worktree without a reason.
- Baseline inspected: `main`, commit `b7a7c62a061c707dd5809d4c401bf36ed286fbbe`, local latest tag `v2.40.6`. Revalidate at execution. No future version number or release SHA is prescribed.
- Follow this repository's `AGENTS.md`. The user explicitly requested Julie revival, so its old maintenance-only guidance does not prevent this work.
- Do not restore `ExtractorManager`, `Symbol.code_context`, public `base`/`pipeline`/`registry`/language implementation modules, or `get_tree_sitter_language`.
- `syntax-api` is off by default. Existing CLI/artifact consumers gain no new required dependency, runtime parse, diagnostics walk, or serialized field.
- Preserve existing extraction contract, identity epoch, SQLite/JSONL schemas, language selection, golden output, and parse-recovery semantics. A syntax-only API addition does not itself justify re-extracting artifacts.
- The optional syntax API never reads or writes the supplied path. It parses exactly the caller's UTF-8 source string; no newline normalization, BOM stripping, canonicalization, or source slicing occurs.
- All spans refer to that exact input. Bytes are zero-based and end-exclusive; diagnostic lines are one-based; diagnostic columns are zero-based UTF-8 byte columns, not UTF-16 units or character counts. Tree-sitter `Point` rows remain zero-based.
- Cover every current language entry and source-sensitive selection case. Do not translate the 40-entry capability inventory into a claim of 40 separate parsers.
- A host tree is not an embedded-language tree forest. JSONL is an explicitly unsupported container for this single-tree API; full canonical JSONL extraction remains supported.
- Syntax errors in a recovered tree are data, not `Err`. Unsupported paths, unsupported containers, an unrepresentable input size, parser setup failure, and no returned tree are errors. Do not silently substitute an empty success or a different language on a parser failure.
- Request callers use `parse_source_with_options`: one shared deadline/cancellation signal covers language probes, parsing, header scoring, and diagnostic traversal. Cancellation/deadline returns an error and discards partial results. Dropping an async wait is not parser cancellation.
- Upstream supplies syntax facts only. Whether an edit may proceed, whether existing errors are tolerated, how before/after diagnostics are compared, and which identifier names to change belong to Julie.
- Keep implementation files at or below 500 lines and new test files at or below 1000 lines. No test comments or narration comments in changed code.
- All task commits use `parallel-lead-commit`: workers do not stage or commit. Root Codex reviews implementation and owns integration. Push/release needs the user's existing or new explicit authorization.

## Current Evidence and Compatibility Boundary

| Existing surface | Verified behavior | Consequence |
|---|---|---|
| `crates/julie-extractors/src/lib.rs:102-125` | Root exports canonical extraction, fact DTOs, `detect_language_for_path`, `detect_language_for_source`, capabilities, and supported language names. | Add named capabilities here; do not reopen modules. |
| `pipeline.rs:66-100` | Canonical extraction special-cases JSONL, then source-sensitive language selection can reuse a winning C/C++ header tree. | The syntax API must not promise one parser invocation for `.h`; it performs two probes and reuses the winner, without a third parse. |
| `pipeline.rs:283-324` | Parser setup uses the path-aware grammar selector; `parse` returns `Option<Tree>`. | Share this path-sensitive grammar choice, especially `.fsi`. A `None` tree is not a successful syntax snapshot. |
| `pipeline.rs:327-347` | Canonical extraction deliberately converts a missing tree into a degraded result with a whole-file diagnostic. | Preserve that contract. The new syntax API has no usable tree and must return `ParseFailed` instead. |
| `pipeline.rs:369-423` | Diagnostics identify ERROR and MISSING nodes; the recursive walker stops at the traversal ceiling. | Do not claim exhaustive syntax diagnostics from that walker. New syntax-only collection must be iterative and complete. |
| `base/span.rs:23-35` | `NormalizedSpan::from_node` uses node byte offsets, one-based lines, and byte columns. | Reuse this conversion without changing coordinates. Reject lengths which cannot fit its `u32` fields. |
| `language_spec/mod.rs:339-406` | `.h` probes both grammars; helper `.ok()?` hides setup failure and missing trees, then detection falls back to C. | Provide a strict internal route for syntax callers. Keep legacy detection's behavior unchanged for existing callers. |
| `language_spec/mod.rs:292-304` | `.fsi` selects `LANGUAGE_SIGNATURE`; other F# paths use the normal grammar. | A public extension-only grammar getter would be the wrong replacement. |
| `base/relationship_resolution.rs:12-69` | Public `StructuredPendingRelationship` has `UnresolvedTarget` and `Option<PendingSpan>` fields. `PendingSpan` aliases `NormalizedSpan`. | Export those two existing names at root so downstream Rust can name the entire fact contract. |
| `docs/plans/2026-09-04-audit-1-hot-path-waste.md:85-103` | Removed symbol context was generated but not used by artifact/CLI output. | Julie should own its body/context representation instead of restoring per-symbol extraction work. |
| `docs/plans/2026-09-04-audit-4-dead-code-and-api-narrowing.md:87-100` | Removed manager was redundant orchestration; canonical extraction replaced it. | No manager shim is needed. |

Julie currently renames by walking live identifier nodes and refusing any pre-existing parse diagnostic in the file (`julie/crates/julie-tools/src/refactoring/mod.rs:362-405`). Its symbol rewrite checks diagnostics touching the target, then uses the tree for body/signature boundaries (`julie/crates/julie-tools/src/editing/rewrite_symbol.rs:329-386,431-502`). Neither inspected preparation method reparses the proposed result. Comparing introduced errors is new consumer hardening, not a behavior this upstream API implements or falsely claims to preserve.

## Public Contract Frozen for the Julie Plan

The following is proposed, not present at the baseline:

```rust
#[cfg(feature = "syntax-api")]
pub mod syntax;

pub use base::relationship_resolution::{PendingSpan, UnresolvedTarget};
```

The module has exactly this initial public interface:

```rust
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use crate::ParseDiagnostic;

#[derive(Debug)]
pub struct ParsedSource {
    pub language: &'static str,
    pub tree: tree_sitter::Tree,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug)]
pub enum SyntaxError {
    UnsupportedLanguage { path: PathBuf },
    UnsupportedContainer { path: PathBuf },
    InputTooLarge { bytes: usize },
    Cancelled,
    DeadlineExceeded,
    ParseFailed { source: anyhow::Error },
}

pub struct SyntaxOptions<'a> {
    pub deadline: Option<Instant>,
    pub cancelled: Option<&'a AtomicBool>,
    pub max_source_bytes: usize,
}

pub fn parse_source(file_path: &Path, source: &str)
    -> Result<ParsedSource, SyntaxError>;

pub fn parse_source_with_options(
    file_path: &Path,
    source: &str,
    options: &SyntaxOptions<'_>,
) -> Result<ParsedSource, SyntaxError>;
```

`SyntaxError` implements `Display` and `std::error::Error`; `ParseFailed::source()` returns the original error. Consumers match variants, never message text. The unsupported container variant initially means a case-insensitive `.jsonl` suffix; document that fact. No filesystem path is converted to lossy text for detection. Return the exact caller-supplied `PathBuf` in path errors.

`ParsedSource` owns its tree and diagnostics, not the source string. The caller must retain the same source when slicing node byte ranges. No public API returns a `Node` detached from its tree. The tree-sitter type is an intentional optional Rust dependency contract; it does not enter SQLite, JSONL, CLI, or non-Rust contracts.

For length validation use a maximum of `(u32::MAX - 1) as usize`; this leaves room for the one-based final line even when every byte is a newline. Unit-test the numeric guard without allocating a multi-gigabyte string.

`SyntaxOptions::default()` has no deadline/cancellation flag and uses the representable byte maximum. `parse_source` delegates with that default; it remains convenient for offline consumers. Request-driven consumers must pass their request deadline, cancellation flag, and an explicit source cap. The effective cap is the minimum of the configured cap and the representable maximum; a zero cap accepts only empty input. Options govern this new API only, never existing canonical extraction defaults.

Cancellation is cooperative: the parser is interrupted through tree-sitter's progress callback and Rust traversals check the same signal. This is not a hard wall-clock guarantee against a stalled external scanner or scheduler. Julie must bound concurrent blocking jobs, signal cancellation on timeout, and retain each job's admission permit until it actually returns; do not accumulate detached `spawn_blocking` tasks or release a slot merely because its awaiting request timed out. A deadline reached after parsing but before diagnostics finish is still `DeadlineExceeded`, not a successful snapshot. Julie checks the same request state again before committing source changes.

**External API grounding, verified 2026-09-07:** tree-sitter 0.26.11 provides [`Parser::parse_with_options`](https://docs.rs/tree-sitter/0.26.11/tree_sitter/struct.Parser.html#method.parse_with_options) with an input callback and optional parse options. [`ParseOptions::progress_callback`](https://docs.rs/tree-sitter/0.26.11/tree_sitter/struct.ParseOptions.html#method.progress_callback) takes `FnMut(&ParseState) -> ControlFlow<()>`; `Break(())` cancels. Do not use a boolean callback or removed timeout/cancellation-pointer APIs. A parser interrupted by a callback can resume on reuse; this API creates and drops its own parser per invocation, so no cancelled parser is retained or reused. Verify these version-pinned signatures again only if the dependency changes.

### Host and container coverage ledger

Every row below must receive a passing fixture-backed syntax contract result at execution. `Host` means the selected grammar's tree covers the original input bytes, not that it includes every separately extracted embedded language. JSX and TSX are capability entries using their existing grammar choices. This ledger is separate from capability claims in `capabilities.json`; do not rewrite that inventory merely to claim this API.

| Entry | Required syntax result | Additional boundary |
|---|---|---|
| bash | Host | Shell strings are not independently parsed program ASTs. |
| c | Host | Include C `.h` and empty `.h`. |
| cpp | Host | Include C++ `.h`; reuse the winning probe tree. |
| csharp | Host | No CLR/typechecking promise. |
| css | Host | No embedded preprocessor parser promise. |
| dart | Host | Existing grammar. |
| elixir | Host | Existing grammar. |
| erlang | Host | Raw syntax diagnostics, not extractor-specific recovery/enrichment diagnostics. |
| fsharp | Host | Test `.fs`, `.fsx`, and signature grammar `.fsi`. |
| gdscript | Host | Existing grammar. |
| go | Host | Existing grammar. |
| html | Host | Script/style contents do not imply embedded trees. |
| java | Host | Existing grammar. |
| javascript | Host | Existing grammar. |
| json | Host | `.json`/`.jsonc` follow existing JSON grammar; `.jsonl` is separately refused. |
| jsx | Host | Existing JavaScript/JSX grammar dispatch. |
| kotlin | Host | Existing grammar. |
| lua | Host | Existing grammar. |
| markdown | Host | Fences do not imply code-language trees. |
| php | Host | Existing PHP grammar including its host constructs. |
| powershell | Host | Existing grammar. |
| python | Host | Existing grammar. |
| qml | Host | Existing QML/JavaScript grammar. |
| qmldir | Host | Extensionless `qmldir`, including case-insensitive basename. |
| r | Host | Existing grammar. |
| razor | Host | Existing Razor grammar; no promise of separate C# trees. |
| regex | Host | Existing grammar. |
| ruby | Host | Existing grammar. |
| rust | Host | Macro contents are only as structured as the existing grammar. |
| scala | Host | Existing grammar. |
| sql | Host | Existing grammar, no database execution. |
| swift | Host | Existing grammar. |
| toml | Host | Existing grammar. |
| tsx | Host | Existing TSX grammar dispatch. |
| typescript | Host | Existing grammar. |
| vbnet | Host | Existing grammar. |
| vue | Host | SFC tree, not separately extracted script ASTs. |
| xml | Host | Include MSBuild extensions from capability metadata. |
| yaml | Host | Existing grammar. |
| zig | Host | Existing grammar. |

Extra path cases: uppercase extensions; Unicode filenames and parent directories; Unix non-UTF-8 parent components; CRLF input; `.h` C/C++ ambiguity; empty supported input; `.jsonl`/`.JSONL`; unsupported `.unknown`; source that produces ERROR/MISSING nodes. The unsupported JSONL host API is not permission to remove canonical JSONL indexing or working text edits in Julie.

## File Structure

| File | Responsibility |
|---|---|
| Modify `crates/julie-extractors/src/lib.rs` | Two named fact exports and feature-gated syntax module. |
| Modify `crates/julie-extractors/Cargo.toml` | Empty optional feature and external integration-test target declarations. |
| Create `crates/julie-extractors/src/syntax/mod.rs` | Public structs/errors and `parse_source` orchestration. |
| Create `crates/julie-extractors/src/syntax/diagnostics.rs` | Iterative host-tree diagnostic collection. |
| Create `crates/julie-extractors/src/language_spec/source_detection.rs` | Move existing source-sensitive/header helpers and add strict failure-propagating route. |
| Modify `crates/julie-extractors/src/language_spec/mod.rs` | Delegate to the moved helpers; retain existing signatures and test counters. |
| Create `crates/julie-extractors/src/tests/public_relationship_contract.rs` | External integration test proving named public fact access. |
| Create `crates/julie-extractors/src/tests/syntax_api_contract.rs` | External integration tests using public syntax API and capabilities only. |
| Create `crates/julie-extractors/src/tests/syntax_api_faults.rs` | Private fault injection and parse-count tests; no public test hooks. |
| Modify `crates/julie-extractors/src/tests/mod.rs` | Feature-gated internal fault-test module registration. |
| Modify `crates/julie-extractors/tests/downstream_smoke.rs` | Existing feature-gated downstream Cargo consumer: optional syntax positive/negative builds. |
| Modify `docs/contracts/extraction-output-changes.md` | Additive optional Rust API change; explicitly unchanged extraction schemas/output. |
| Create `docs/contracts/rust-syntax-api.md` | Supported contract, examples, errors, coordinates, host-only limits, all-entry test evidence. |
| Modify `crates/julie-extractors/README.md` | Link the supported optional API, using root canonical extraction in examples. |

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `xtask/src/test_tiers.rs`, and `crates/julie-extractors/Cargo.toml`.

**Worker red/green scope:** Exact named `cargo test -p julie-extractors` filters specified below. Each behavior gets one observed RED and one observed GREEN. Do not run concurrent Cargo commands.

**Worker ceiling:** Assigned exact tests only, plus the assigned downstream smoke test when its task runs. No xtask tiers from workers.

**Worker gate invariant:** Fact types are nameable outside the crate; parsing returns the correct host tree and exhaustive byte-coordinate diagnostics; parser failures/cancellation cannot masquerade as supported clean syntax; feature-off downstream compilation remains unchanged. In-flight cancellation reaches the actual parser and diagnostic walker, not only their caller's wait.

**Lead affected-change scope:** Once per completed code batch: `cargo xtask test changed crates/julie-extractors/Cargo.toml crates/julie-extractors/src/language_spec/source_detection.rs crates/julie-extractors/src/language_spec/mod.rs crates/julie-extractors/src/syntax/mod.rs crates/julie-extractors/src/syntax/diagnostics.rs`. Current changed-path rules escalate manifest/language-spec edits to certification; accept the declared scope rather than silently substituting a small filter.

**Branch gate:** `cargo xtask test contract`, plus `cargo test -p julie-extractors --features syntax-api --test syntax_api_contract`, `cargo test -p julie-extractors --features syntax-api --lib tests::syntax_api_faults`, and `cargo test -p julie-extractors --features syntax-api --doc`. Reuse exact-HEAD matching gate evidence rather than repeating it. Run `cargo fmt --all -- --check`.

**Security scope:** `cargo deny check` (documented dependency-policy gate); no dedicated secrets-scan command is declared in this repo's testing strategy.

**Replay/metric evidence:** Hard gates: unchanged goldens without regeneration, exact capability-entry coverage, zero default-feature syntax symbols accessible downstream, no new dependency, no third `.h` parse. Report-only: cold/warm syntax-test wall time and release binary size comparison on the same platform/toolchain/profile. Do not make a byte-identical release binary a gate: compiler/layout changes can change size despite identical executed paths.

**Escalation triggers:** Parser dependency or grammar changes require certification (already selected by these paths); unintended extraction-output changes require diagnosis and correction, not golden regeneration. Run public syntax and path tests on Windows before the release handoff, because this is a public path API. Do not run store crash/real-world suites solely for a Rust syntax adapter.

**Assigned verification failure:** Diagnose and fix failures in scope; if a required public-contract change contradicts this plan, report the mismatch before changing it. Do not weaken assertions to pass.

**Verification ledger:** Record invariant, exact command, scope label, commit SHA, timestamp, result, and dirty-diff identity. Reuse only evidence for the same scope and exact HEAD; uncommitted implementation changes invalidate earlier evidence even when HEAD is unchanged. The lead records the final verified implementation SHA before handing it to Julie.

## Verification Ledger

No implementation verification has run. Add rows only when the corresponding command has executed; a plan review is not a passing runtime gate.

| Invariant | Exact command | Scope label | Commit SHA | Dirty-diff identity | Timestamp | Result | Evidence path |
|---|---|---|---|---|---|---|---|

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Name relationship facts | None - serial | `src/lib.rs`, `Cargo.toml`, `src/tests/public_relationship_contract.rs` under `crates/julie-extractors/` | Yes | Public exports and manifest are shared with Task 2. |
| Task 2: Complete optional syntax API | None - serial | `src/lib.rs`, `Cargo.toml`, `src/syntax/mod.rs`, `src/syntax/diagnostics.rs`, `src/language_spec/mod.rs`, `src/language_spec/source_detection.rs`, `src/tests/mod.rs`, `src/tests/syntax_api_contract.rs`, `src/tests/syntax_api_faults.rs` under `crates/julie-extractors/` | Yes | Depends on Task 1 manifest/export changes and owns one cohesive public parse contract. |
| Task 3: Downstream feature proof and documentation | None - serial | `crates/julie-extractors/tests/downstream_smoke.rs`, `crates/julie-extractors/README.md`, `docs/contracts/rust-syntax-api.md`, `docs/contracts/extraction-output-changes.md` | Yes | Consumes the completed API and must use the final reviewed contract. |

Root Codex is the final implementation reviewer. Each worker supplies path, branch, HEAD, dirty state, exact owned-file diff, RED/GREEN command output, and any unresolved evidence gap. No worker stages, commits, tags, releases, or edits the adjacent Julie checkout. Complete a task's review and buildable handoff before beginning the next; these boundaries are not permission pauses.

## Task 1: Name Relationship Fact Types Outside the Crate

**Files:** Modify `crates/julie-extractors/src/lib.rs:102-110` and `crates/julie-extractors/Cargo.toml`; create test `crates/julie-extractors/src/tests/public_relationship_contract.rs`.

**Interfaces:** Consumes existing `StructuredPendingRelationship`, `UnresolvedTarget`, `PendingSpan = NormalizedSpan`. Produces root public names `UnresolvedTarget` and `PendingSpan`; no changed representation or serializer.

**Contract inputs:** The exact public contract above; current `relationship_resolution.rs` struct fields. Existing internal modules remain `pub(crate)`.

**File ownership:** `src/lib.rs`, `Cargo.toml`, `src/tests/public_relationship_contract.rs` under `crates/julie-extractors/`.

**Serialization required:** Yes.

**Dependency reason:** Public exports and manifest are shared with Task 2.

**Step 1: Write this proposed external test and register it.** This file is a Cargo integration-test crate despite residing under `src/tests/`; do not register it as an internal unit-test module.

```toml
[[test]]
name = "public_relationship_contract"
path = "src/tests/public_relationship_contract.rs"
```

```rust
use julie_extractors::{NormalizedSpan, PendingSpan, UnresolvedTarget};

#[test]
fn relationship_components_are_nameable_by_external_consumers() {
    let target = UnresolvedTarget::simple("worker.run");
    let span: PendingSpan = NormalizedSpan {
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 10,
        start_byte: 0,
        end_byte: 10,
    };
    assert_eq!(target.display_name, "worker.run");
    assert_eq!(span.end_byte, 10);
}
```

**Step 2: Observe RED.** Run `cargo test -p julie-extractors --test public_relationship_contract relationship_components_are_nameable_by_external_consumers -- --exact`. Expected failure: unresolved root imports, not a fixture/path error.

**Step 3: Implement the minimal export.** Proposed addition to `lib.rs`:

```rust
pub use base::relationship_resolution::{PendingSpan, UnresolvedTarget};
```

Do not export the module. The existing `base` module can remain private because a public item can be re-exported through it. Check nested signatures: these two types require only existing `NormalizedSpan`, primitive fields, and standard containers; they do not require a larger API expansion.

**Step 4: Observe GREEN.** Repeat the exact command. The test must compile as an external crate, not through `crate::base` visibility.

**Step 5: Apply commit mode.** `parallel-lead-commit`: hand the verified owned diff and evidence to Root Codex; do not stage or commit.

**Acceptance criteria:**
- [ ] Both existing value types are nameable through root imports externally.
- [ ] Internal modules remain private and no serialized output changes.
- [ ] Exact RED/GREEN evidence and a buildable owned diff are handed to the lead.

## Task 2: Implement the Complete Optional Host Syntax API

**Files:** Create `crates/julie-extractors/src/syntax/{mod,diagnostics}.rs`, `src/language_spec/source_detection.rs`, `src/tests/{syntax_api_contract,syntax_api_faults}.rs`; modify `src/lib.rs`, `Cargo.toml`, `src/language_spec/mod.rs`, and `src/tests/mod.rs` in that crate.

**Interfaces:** Consumes current path-aware grammar configuration, normalized span DTOs, capability snapshots, and Task 1 exports. Produces the exact `parse_source`/`ParsedSource`/`SyntaxError` contract above; no extra parser getter or extraction manager.

**Contract inputs:** Every global constraint and every coverage-ledger row. `detect_language_with_tree` and canonical extraction retain their existing interfaces and fallback behavior; only the new strict route propagates header probe failures.

**File ownership:** `src/lib.rs`, `Cargo.toml`, `src/syntax/mod.rs`, `src/syntax/diagnostics.rs`, `src/language_spec/mod.rs`, `src/language_spec/source_detection.rs`, `src/tests/mod.rs`, `src/tests/syntax_api_contract.rs`, `src/tests/syntax_api_faults.rs` under `crates/julie-extractors/`.

**Serialization required:** Yes.

**Dependency reason:** Depends on Task 1 manifest/export changes and owns one cohesive public parse contract.

**Step 1: Register the optional feature/test target and write the public test cases before implementation.**

```toml
[features]
syntax-api = []

[[test]]
name = "syntax_api_contract"
path = "src/tests/syntax_api_contract.rs"
required-features = ["syntax-api"]
```

Add the feature to the existing `[features]` table, not a duplicate table. Start the external target with this proposed test:

```rust
use std::path::Path;
use julie_extractors::syntax::{parse_source, SyntaxError};

#[test]
fn syntax_api_returns_tree_and_recovery_diagnostics() {
    let source = "fn compute() { let answer = 42; }\n";
    let parsed = parse_source(Path::new("unicode/λ.rs"), source).unwrap();
    assert_eq!(parsed.language, "rust");
    assert!(parsed.diagnostics.is_empty());
    let function = parsed.tree.root_node().named_child(0).unwrap();
    let body = function.child_by_field_name("body").unwrap();
    assert_eq!(&source[body.byte_range()], "{ let answer = 42; }");
    assert_eq!(&source[function.start_byte()..body.start_byte()], "fn compute() ");

    let malformed = "fn broken( {\n";
    let recovered = parse_source(Path::new("broken.rs"), malformed).unwrap();
    assert!(recovered.tree.root_node().has_error());
    assert!(!recovered.diagnostics.is_empty());
    assert!(recovered.diagnostics.iter().all(|d| {
        d.start_byte <= d.end_byte && d.end_byte as usize <= malformed.len()
    }));
    assert!(matches!(parse_source(Path::new("x.unknown"), "x"),
        Err(SyntaxError::UnsupportedLanguage { .. })));
    assert!(matches!(parse_source(Path::new("x.JSONL"), "{}\n{}\n"),
        Err(SyntaxError::UnsupportedContainer { .. })));
}
```

Add these independently named tests in the same external target, with these exact assertions:

- `syntax_api_preserves_unicode_crlf_byte_coordinates`: parse `"fn café() { let x = ; }\r\n"`; require diagnostics; recompute each diagnostic's one-based line and byte column from `source.as_bytes()[..start_byte]` and repeat for `end_byte`; verify `source.is_char_boundary` for both offsets. Do not compare columns to `.chars().count()`.
- `syntax_api_handles_missing_nodes`: use malformed Rust fixtures to exercise `is_missing()` as well as ERROR. Require at least one `ParseDiagnosticKind::Missing` from a fixture which demonstrably produces a missing node under the pinned parser; identify that fixture with a narrow parse probe if the proposed missing semicolon sample does not. The test must assert the exact recovered-node span, including zero width, rather than merely nonempty diagnostics.
- `syntax_api_selects_source_sensitive_grammars`: table-test C `.h`, C++ `.h`, empty `.h`, uppercase `.H`, extensionless `qmldir`, `.fs`, `.fsx`, and `.fsi`. Reuse the C/C++ samples in `src/tests/pipeline.rs:266-374`; the F# signature sample must include a `val` declaration parsed by the signature grammar, not just empty input.
- `syntax_api_covers_every_supported_entry`: use the complete executable example below. No hard-coded three-language subset.
- `syntax_api_reports_host_only_composition`: select the existing Vue, Markdown, HTML, and Razor basic fixtures from `capability_snapshot`. Require `parsed.language` to match the entry and every node/diagnostic range to reference the exact full input. Compare their root kinds with direct existing grammar parses in the private test, not with a claim that separately extracted script/fence nodes exist.
- `syntax_api_allows_empty_supported_source`: `parse_source("empty.rs", "")` returns a tree with no diagnostics.
- `syntax_api_rejects_cancelled_or_expired_requests`: the complete proposed test below verifies the public variants and configurable input cap before parsing starts.

```rust
#[test]
fn syntax_api_rejects_cancelled_or_expired_requests() {
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;
    use julie_extractors::syntax::{parse_source_with_options, SyntaxOptions};

    let cancelled = AtomicBool::new(true);
    let options = SyntaxOptions {
        cancelled: Some(&cancelled),
        deadline: None,
        max_source_bytes: 1024,
    };
    assert!(matches!(parse_source_with_options(Path::new("x.rs"), "", &options),
        Err(SyntaxError::Cancelled)));
    let options = SyntaxOptions {
        cancelled: None,
        deadline: Some(Instant::now()),
        max_source_bytes: 1024,
    };
    assert!(matches!(parse_source_with_options(Path::new("x.rs"), "", &options),
        Err(SyntaxError::DeadlineExceeded)));
    let options = SyntaxOptions { max_source_bytes: 2, ..SyntaxOptions::default() };
    assert!(matches!(parse_source_with_options(Path::new("x.rs"), "fn f() {}", &options),
        Err(SyntaxError::InputTooLarge { .. })));
}
```

The complete proposed coverage test deliberately uses declared fixture paths instead of inventing language examples:

```rust
#[test]
fn syntax_api_covers_every_supported_entry() {
    use std::collections::BTreeSet;
    use julie_extractors::{capability_snapshot, supported_languages};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let expected: BTreeSet<_> = supported_languages().into_iter().collect();
    let mut seen = BTreeSet::new();
    let snapshot = capability_snapshot();
    for row in snapshot.languages() {
        let fixture = row.fixtures.iter().find(|f| {
            f.name == "basic" && !f.source.ends_with(".jsonl")
        }).or_else(|| row.fixtures.iter().find(|f| !f.source.ends_with(".jsonl")))
            .expect("every language entry needs a non-JSONL host fixture");
        let path = root.join(&fixture.source);
        let source = std::fs::read_to_string(&path).unwrap();
        let parsed = parse_source(&path, &source).unwrap();
        assert_eq!(parsed.language, row.language, "{}", path.display());
        for diagnostic in &parsed.diagnostics {
            assert!(diagnostic.start_byte <= diagnostic.end_byte);
            assert!(diagnostic.end_byte as usize <= source.len());
        }
        seen.insert(row.language.as_str());
    }
    assert_eq!(seen, expected);
}
```

If a capability fixture has a `.h` suffix but intentionally declares a different language than automatic detection, add an explicit path-selection fixture for that entry and retain the automatic-detection contract; do not force the requested language through a hidden override. Preserve all 40 current entries and fail visibly on future inventory drift. Record the actual selected fixture per row in the evidence document.

**Step 2: Observe RED.** Run `cargo test -p julie-extractors --features syntax-api --test syntax_api_contract syntax_api_returns_tree_and_recovery_diagnostics -- --exact`. Expected failure: public module/function missing. For each additional independent behavior, record its own exact named command before its implementation. A compilation failure caused by the intentionally missing API is valid RED; unrelated build failure is not.

**Step 3: Implement the public adapter and strict detection.** The proposed orchestration is:

```rust
pub fn parse_source(file_path: &Path, source: &str)
    -> Result<ParsedSource, SyntaxError>
{
    parse_source_with_options(file_path, source, &SyntaxOptions::default())
}

pub fn parse_source_with_options(
    file_path: &Path,
    source: &str,
    options: &SyntaxOptions<'_>,
) -> Result<ParsedSource, SyntaxError> {
    options.check()?;
    validate_source_len(source.len(), options.max_source_bytes)?;
    if file_path.extension().and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
    {
        return Err(SyntaxError::UnsupportedContainer { path: file_path.into() });
    }
    let (language, pre_parsed) =
        crate::language_spec::source_detection::detect_strict(file_path, source, options)?
            .ok_or_else(|| SyntaxError::UnsupportedLanguage { path: file_path.into() })?;
    let tree = match pre_parsed {
        Some(tree) => tree,
        None => {
            let grammar = crate::language_spec::get_tree_sitter_language_for_path(
                language, file_path,
            ).map_err(|source| SyntaxError::ParseFailed { source })?;
            parse_tree_with_options(&grammar, source, options)?
        }
    };
    let diagnostics = diagnostics::collect(&tree, options)?;
    options.check()?;
    Ok(ParsedSource { language, tree, diagnostics })
}

pub(crate) fn validate_source_len(bytes: usize, configured_max: usize)
    -> Result<(), SyntaxError>
{
    if bytes > configured_max.min((u32::MAX - 1) as usize) {
        return Err(SyntaxError::InputTooLarge { bytes });
    }
    Ok(())
}
```

Implement `Display` by variant, including the path for unsupported errors and the byte count for oversize; `Error::source` yields `Some(source.as_ref())` only for `ParseFailed`. This preserves the underlying cause instead of flattening it into a successful empty parse. The new private `SyntaxOptions::check` returns `Cancelled` if its atomic flag is set (Acquire load), then `DeadlineExceeded` if `Instant::now() >= deadline`, otherwise success. Check before setup, after setup, before returning, and throughout traversal. Cancellation takes precedence if both conditions hold.

The proposed parsing helper uses the verified progress API and preserves the cancellation reason after the callback returns:

```rust
pub(crate) fn parse_tree_with_options(
    grammar: &tree_sitter::Language,
    source: &str,
    options: &SyntaxOptions<'_>,
) -> Result<tree_sitter::Tree, SyntaxError> {
    use std::ops::ControlFlow;
    options.check()?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(grammar)
        .map_err(|e| SyntaxError::ParseFailed { source: e.into() })?;
    options.check()?;
    let mut stopped = None;
    let tree = {
        let mut progress = |_: &tree_sitter::ParseState| {
            if let Err(error) = options.check() {
                stopped = Some(error);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let bytes = source.as_bytes();
        let mut input = |offset: usize, _: tree_sitter::Point| {
            bytes.get(offset..).unwrap_or(&[])
        };
        parser.parse_with_options(
            &mut input,
            None,
            Some(tree_sitter::ParseOptions::new().progress_callback(&mut progress)),
        )
    };
    if let Some(error) = stopped {
        return Err(error);
    }
    options.check()?;
    tree.ok_or_else(|| SyntaxError::ParseFailed {
        source: anyhow::anyhow!("parser returned no tree"),
    })
}
```

Move the existing source detection and C/C++ helpers from `language_spec/mod.rs` into private `source_detection.rs`; keep `language_spec` and this child module crate-private. Preserve the public root detection entrypoints and existing test-counter accessors by delegation. Preserve the existing C/C++ tie-breaker and error-count policy exactly. Introduce this proposed shared shape rather than copying the whole detection algorithm into `syntax`:

```rust
type DetectedTree = (&'static str, Option<tree_sitter::Tree>);

#[cfg(feature = "syntax-api")]
pub(crate) fn detect_strict(
    path: &Path,
    source: &str,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<Option<DetectedTree>, crate::syntax::SyntaxError>
{
    detect_with_probe(path, source, |source| bounded_header_probe(source, options).map(Some))
}

pub(crate) fn detect_legacy(path: &Path, source: &str) -> Option<DetectedTree> {
    detect_with_probe(path, source, |source| {
        Ok::<_, std::convert::Infallible>(strict_header_probe(source).ok())
    })
        .expect("legacy header adapter is infallible")
}
```

`detect_with_probe` contains the existing basename/extension/header rules once. Make it generic over callback error type `E`: its callback returns `Result<Option<(&'static str, Tree)>, E>` and the function returns `Result<Option<DetectedTree>, E>`. Only `None` for a completed optional legacy probe permits the old C fallback. The bounded callback propagates `SyntaxError` unchanged, including cancellation/deadline; do not wrap them into `ParseFailed`. The legacy strict probe uses the existing ranking/tie-breaker with contextual `Result` failures instead of `.ok()?`; its adapter preserves the current fallback behavior.

Factor the shared C/C++ choice so bounded and legacy routes do not drift: the bounded route parses both grammars with `parse_tree_with_options`, carries the same options through error-count traversal, and checks at least every 256 consumed characters in comment/string stripping and tie-break scanning. Both routes retain the existing error-count depth policy and tie-break order; a bounded cancellation returns an error instead of a partial score or partial code string. Check between the two probes and before selecting the winner. The default route uses its existing non-cancellable behavior and does not create options or read clocks. Never run `.h` through unbounded `header_parse_prefers_cpp` from the bounded API. If factoring requires additional private helper signatures, keep their errors generic or preserve `SyntaxError` directly; no shared parser primitive must depend on the optional public module when the feature is disabled.

For diagnostics use an iterative depth-first cursor walk that inspects ERROR and MISSING nodes without a depth cutoff. The following proposed implementation uses only tree operations already used in this repository, with `goto_*` traversal replacing recursion:

```rust
pub(super) fn collect(
    tree: &tree_sitter::Tree,
    options: &super::SyntaxOptions<'_>,
) -> Result<Vec<crate::ParseDiagnostic>, super::SyntaxError> {
    use crate::{NormalizedSpan, ParseDiagnostic, ParseDiagnosticKind};
    let mut cursor = tree.walk();
    let mut diagnostics = Vec::new();
    loop {
        options.check()?;
        let node = cursor.node();
        for (present, kind) in [
            (node.is_error(), ParseDiagnosticKind::Error),
            (node.is_missing(), ParseDiagnosticKind::Missing),
        ] {
            if present {
                let span = NormalizedSpan::from_node(&node);
                diagnostics.push(ParseDiagnostic {
                    kind,
                    message: None,
                    start_line: span.start_line,
                    start_column: span.start_column,
                    end_line: span.end_line,
                    end_column: span.end_column,
                    start_byte: span.start_byte,
                    end_byte: span.end_byte,
                });
            }
        }
        if node.has_error() && cursor.goto_first_child() {
            continue;
        }
        loop {
            options.check()?;
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(diagnostics);
            }
        }
    }
}
```

This does not alter the existing extraction diagnostic collector or add extractor-specific diagnostic messages. Use the same tree-sitter crate instance already pinned by this crate; do not add a second version. If the compiler exposes an ownership difference in the example, adapt locally while retaining the exact public contract and traversal invariants.

Before accepting GREEN, add private fault tests in `src/tests/syntax_api_faults.rs` and register that unit module under `#[cfg(feature = "syntax-api")]`. Put injectability at the narrow parsing/probe helper boundary, not a public callback. Test names and exact behavior:

| Exact unit-test name under `tests::syntax_api_faults` | Injection/assertion |
|---|---|
| `strict_header_probe_propagates_failure` | Inject `Err(anyhow!("probe sentinel"))` into `detect_with_probe` for nonempty `.h`; require `Err` with the sentinel cause, never C success. |
| `syntax_parser_none_is_parse_failed` | Inject `Ok(None)` into the private parser completion helper; require `SyntaxError::ParseFailed`. |
| `syntax_parser_setup_error_preserves_cause` | Inject parser-setup `Err` and assert `std::error::Error::source()` is present. No fabricated invalid grammar pointer/unsafe code. |
| `syntax_source_length_guard_does_not_truncate` | Numeric checks at max, max+1, and zero; do not allocate max-sized source. |
| `syntax_header_reuses_winning_tree` | Use existing header-probe counters and a syntax parse-completion counter: nonempty `.h` probes twice and does not parse again; normal `.rs` parses once; empty `.h` parses once. |
| `syntax_diagnostics_include_deep_recovery` | Build a deeply nested malformed syntax sample beyond the existing traversal limit; run on a bounded-stack thread, require ERROR/MISSING diagnostics and valid byte spans without a stack overflow. Do not assume a specific malformed shape works: prove its tree has a nested error, then assert the API reports that node. |
| `syntax_path_keeps_non_utf8_parents` | Unix-only path with invalid UTF-8 parent bytes and a valid `.rs` filename still selects Rust. Windows runs the Unicode/path variant instead. |
| `syntax_cancellation_reaches_parser_progress` | A private progress-observer seam flips the request atomic after the real parser invokes progress; require `Cancelled` from the real parser helper and prove progress ran. Do not test only a pre-cancelled call or a dropped async wait. |
| `syntax_deadline_reaches_parser_progress` | Use a test clock seam advancing past the deadline after a real progress callback; require `DeadlineExceeded`. Avoid timing-sensitive sleeps as the sole assertion. |
| `syntax_cancellation_stops_header_probes` | Trigger cancellation in the first real header probe; require `Cancelled`, no second probe, no fallback C result. Also cancel between probes and during header scoring. |
| `syntax_cancellation_stops_diagnostic_walk` | Trigger cancellation after a known number of cursor visits through a private test hook; require `Cancelled`, not a partial `ParsedSource`. |
| `syntax_deadline_after_parse_rejects_result` | Expire a test clock after tree production but before diagnostics finish; require `DeadlineExceeded` and no successful snapshot. |

A proposed exact fault-test body, using the new internal helper rather than mutating global state:

```rust
#[test]
fn strict_header_probe_propagates_failure() {
    let result = crate::language_spec::source_detection::detect_with_probe(
        std::path::Path::new("sample.h"),
        "int answer(void);",
        |_| Err(anyhow::anyhow!("probe sentinel")),
    );
    assert!(result.unwrap_err().to_string().contains("probe sentinel"));
}
```

Keep helper visibility `pub(crate)` only where sibling tests need it; public consumers see only the frozen interface. Test hooks are private, feature/test-gated, and inject observation/clock behavior at the production helper's actual decision points; never add a public arbitrary callback which can itself block. The helper that converts parser completion to a `Tree` must be used by production so fault injection verifies a real decision, not a duplicate test implementation.

**Step 4: Observe GREEN.** Repeat the initial exact test. Run each added public test with `cargo test -p julie-extractors --features syntax-api --test syntax_api_contract <exact-name> -- --exact`, and each fault test with `cargo test -p julie-extractors --features syntax-api --lib tests::syntax_api_faults::<exact-name> -- --exact`. These are concrete names listed above, not broad language suites. Capture the initial RED and final GREEN once per behavior. The lead owns broad/regression gates.

**Step 5: Apply commit mode.** `parallel-lead-commit`; hand the complete buildable feature, full inventory ledger, and exact test evidence to Root Codex. This task cannot be accepted with only Rust/C#/Python tested or with hidden JSONL/composite fallbacks.

**Acceptance criteria:**
- [ ] Public interface exactly matches the frozen cross-plan contract and is feature-gated.
- [ ] Unsupported path/container, oversized source, parser setup failure, and no-tree behavior are distinct from recovery diagnostics.
- [ ] Header selection matches existing behavior on successful parsing and never reparses the winner.
- [ ] `.fsi`, extensionless qmldir, uppercase extensions, and all 40 current entries have evidence.
- [ ] Diagnostics include deep ERROR/MISSING nodes with byte-correct Unicode/CRLF spans and no recursive stack exhaustion.
- [ ] In-flight cancellation/deadline reaches both header probes, parser progress, header scoring, and diagnostics; no partial success or detached worker is mistaken for cancellation.
- [ ] Canonical extraction, public detection compatibility, identity/version markers, and golden output are unchanged.
- [ ] Host-only boundaries and JSONL refusal are explicit; no blanket embedded-language editing claim.
- [ ] Default feature graph gains no dependency and default execution performs no syntax-API work.
- [ ] Every exact RED/GREEN result, path/branch/HEAD/dirty state, and owned diff is delivered to the lead.

## Task 3: Prove the Downstream Feature Boundary and Document the Release Contract

**Files:** Modify `crates/julie-extractors/tests/downstream_smoke.rs`, `crates/julie-extractors/README.md`, and `docs/contracts/extraction-output-changes.md`; create `docs/contracts/rust-syntax-api.md`.

**Interfaces:** Consumes Tasks 1-2 exactly. Produces a separate Cargo consumer proof and a release handoff the Julie migration can pin.

**Contract inputs:** Existing `test-downstream-smoke` gate and `cargo_path_dependency`/Windows prefix helper in `tests/downstream_smoke.rs`. The public Rust feature does not change the primary CLI/artifact boundary.

**File ownership:** `crates/julie-extractors/tests/downstream_smoke.rs`, `crates/julie-extractors/README.md`, `docs/contracts/rust-syntax-api.md`, `docs/contracts/extraction-output-changes.md`.

**Serialization required:** Yes.

**Dependency reason:** Consumes the completed API and must use the final reviewed contract.

**Step 1: Add a downstream test before changing its fixture program.** Extend the existing feature-gated smoke file with `syntax_api_is_optional_for_external_consumers`. Use a temporary consumer outside the workspace, the existing path escaping helper, and a separate target directory. Reuse that target directory within the one test; do not run nested Cargo against the parent's locked target directory.

The proposed positive fixture program is complete:

```rust
use std::path::Path;
use julie_extractors::{PendingSpan, UnresolvedTarget};
use julie_extractors::syntax::{parse_source, SyntaxError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "fn update() { let item = 1; }";
    let parsed = parse_source(Path::new("src/lib.rs"), source)?;
    assert!(parsed.diagnostics.is_empty());
    let function = parsed.tree.root_node().named_child(0).unwrap();
    let body = function.child_by_field_name("body").unwrap();
    assert_eq!(&source[body.byte_range()], "{ let item = 1; }");
    let _target: UnresolvedTarget = UnresolvedTarget::simple("update");
    let _span: Option<PendingSpan> = None;
    assert!(matches!(parse_source(Path::new("data.jsonl"), "{}\n{}"),
        Err(SyntaxError::UnsupportedContainer { .. })));
    Ok(())
}
```

The temporary manifest enables `julie-extractors = { path = <escaped local crate path>, default-features = false, features = ["syntax-api"] }`. Render it with the existing helper; do not interpolate an unescaped host path. The fixture can use inferred tree types without adding its own tree-sitter dependency.

**Step 2: Verify the intended failure boundary.** In the same test, first compile that fixture with `default-features = false` and without `syntax-api`. Capture Cargo stderr and require a failure identifying the missing `syntax` module; dependency/network failure is not a passing negative test. Then compile/run the same program with `syntax-api` enabled and require success. Finally restore the original default-feature canonical-extraction fixture and require success. These checks deliberately test absence/presence, not production stubs.

Run the exact new test with `cargo test -p julie-extractors --features test-downstream-smoke --test downstream_smoke syntax_api_is_optional_for_external_consumers -- --exact`. This is a verification/documentation task after an implemented API: expected behavior is already green; do not manufacture a production defect solely to create a RED. Its negative feature-off build is the required observed rejection.

**Step 3: Implement the consumer fixture transitions and write the contract document.** The smoke test must execute all three builds in order and assert their outcomes; do not treat a skipped/zero-test run as proof. Include this manifest/API example in the new document, marking the dependency version as the actual release chosen during execution rather than inventing a future tag:

```toml
[dependencies]
julie-extractors = { path = "../julie-extractors/crates/julie-extractors", default-features = false, features = ["syntax-api"] }
```

The contract document must contain both frozen signatures, `SyntaxOptions` defaults, complete error table, cooperative cancellation limits, bounded-worker admission/join requirement, ownership/lifetime rule, byte/line/column table, `.h` probe count, `.fsi` dispatch, JSONL refusal, host/composite limits, fixture-per-entry ledger, and a diagnostic example explaining that a recovered tree is not an edit approval. Include the current public capability extension lookup `capability_snapshot().languages()` so consumers do not recreate an extension registry. Extend the external fixture to construct `SyntaxOptions`, invoke `parse_source_with_options`, and match both interruption variants so the bounded contract is nameable outside the crate too.

Add an unreleased Rust-API entry to `extraction-output-changes.md`: two existing root fact exports and optional host syntax; unchanged extraction contract/epoch/schema/output; any dependency release version is assigned only during authorized release execution. Do not call the plan complete merely because documentation exists.

**Step 4: Verify GREEN and lead gates.** Run the exact downstream test once after its implementation. Root Codex runs the Verification Strategy gates, reviews the feature-off dependency graph, and checks that the syntax module is entirely feature-gated and has no callers from default extraction. Record report-only release binary sizes if built; never claim a measured performance improvement from static feature inspection.

**Step 5: Apply commit mode and prepare release handoff.** `parallel-lead-commit`. Worker does not release. The lead records the verified implementation commit, contract marker, identity epoch, tested platforms/toolchains, exact Cargo feature, and test ledger. An authorized release supplies the real published tag and corresponding SHA to Julie. If release approval is absent, hand off the reviewed commit for local/path testing and explicitly mark publication pending; do not invent a pin.

**Acceptance criteria:**
- [ ] Separate Cargo consumer proves feature-off rejection, feature-on success, and unchanged default canonical extraction.
- [ ] The syntax test cannot pass by running zero tests or by treating a dependency failure as expected feature absence.
- [ ] Public docs define every error/coordinate/ownership/container/composition behavior and contain the full fixture ledger.
- [ ] No old manager, source-context field, public private-module shortcut, or public grammar-loader API returns.
- [ ] Root Codex reviewed implementation and verified all required gates on the handed-off source state.
- [ ] Windows path/consumer evidence is present before a release handoff; missing access is reported as a real blocker, not a false pass.
- [ ] Julie receives the actual reviewed commit/tag plus `syntax-api` contract, not a guessed future release number.

## Final Worker-to-Lead Handoff

Supply one packet with: repository/worktree path; branch; full commit SHA; `git status --short --branch`; `git worktree list`; related worktree dirty states; owned-file diff; task checkboxes; exact RED/GREEN output; lead-gate ledger; capability ledger with selected fixture per entry; default-feature graph proof; Windows result; unresolved blockers; and the actual release state. Root Codex checks it against the adjacent Julie migration plan before accepting completion.

Planning-session state: this document alone is authorized in this worker lane. No implementation, tests, staging, commit, push, tag, or release has been performed by writing it.
