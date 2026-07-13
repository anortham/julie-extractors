# Razor Parser Contract Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Consume the hardened Razor grammar at an exact commit, eliminate stale node-kind assumptions, and prove current Razor/Blazor extraction stays useful and diagnostic-free.

**Architecture:** Keep Julie Extractors policy-free and parser-backed: the grammar owns syntax, Julie owns per-file symbols/facts and artifact evidence, and Miller owns workspace inheritance. Centralize only the private expression-node predicate that two Razor collectors already need; do not add a new structural-fact family or infer `_Imports.razor` inheritance inside a per-file extractor.

**Tech Stack:** Rust, tree-sitter 0.26, git-pinned `tree-sitter-razor`, Julie extraction/artifact contracts, golden fixtures, xtask verification tiers.

**Architecture Quality:** Affected modules are the Razor symbol visitor, Razor structural-fact collector, tests/fixtures, parser pin, and certification evidence. Caller-facing interfaces are `ExtractionResults` and existing versioned artifact facts. Risk is medium because a parser pin can change every Razor tree while the intended extractor change is narrow. Keep node-kind knowledge behind one private predicate, test through `extract_canonical`, reject workspace-global namespace resolution here, and reject error-recovery-only assertions for syntax the grammar now claims to support.

## Global Constraints

- This plan starts only after `/Users/murphy/source/tree-sitter-razor/docs/plans/2026-07-13-current-razor-and-bindings-hardening.md` finishes with a clean, verified 40-character commit SHA.
- Pin the exact grammar commit; never use a branch, tag, or floating git dependency.
- Preserve existing pattern IDs and metadata schemas; no new structural-fact family is needed.
- Per-file extraction must not infer `_Imports.razor`, project, folder, external/internal component, or workspace-global state.
- `_Imports.razor` directive symbols and paths remain raw evidence for Miller to compose.
- Fast tests remain in the default/language tier; real repositories, parser certification, and dogfood remain outside the default suite.
- `silent_cells` and `quality_bar_debts` must both remain `0`.
- Do not edit tree-sitter-razor, Miller, or Eros from this repository.
- Do not publish, push, tag, or release without explicit approval.
- Preserve the unrelated untracked `docs/plans/2026-07-11-tsql-parse-quality-implementation-plan.md` and every existing worktree.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `fixtures/extraction/capabilities.json`, existing Razor golden fixtures, and artifact/structural-fact contracts.

**Worker red/green scope:** Focused Rust unit tests using `cargo test -p julie-extractors <test-name>` and `cargo xtask test language razor` if the live xtask list exposes the Razor selector.

**Worker ceiling:** Workers run focused tests and the Razor language tier. The lead owns changed-path, golden, capability, contract, certification, real-world, and dogfood gates.

**Worker gate invariant:** Explicit `@(…)` inputs must emit the existing expression symbol/fact behavior; valid current Razor inputs must have empty `parse_diagnostics`; malformed recovery inputs must preserve following valid facts without claiming a clean parse.

**Lead affected-change scope:** `cargo xtask test changed crates/julie-extractors/Cargo.toml crates/julie-extractors/src/razor/mod.rs crates/julie-extractors/src/base/framework_structural_facts/razor.rs fixtures/extraction/razor`.

**Branch gate:** `cargo xtask test default`, Razor language tests, `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract`, `cargo xtask test certification`, strict language data-quality report, focused real-world/dogfood evidence from Task 4, `cargo deny check`, and `cargo xtask release package-list`.

**Replay/metric evidence:** Hard gates are zero unit/golden/contract failures, `silent_cells=0`, `quality_bar_debts=0`, all live-counted Terraform Razor files processed (`N/N`), zero failed parses, and zero Razor parse diagnostics. Record `N` from the current corpus; investigate and report drift from the prior count of 69 instead of forcing the old count. Timing and throughput are report-only.

**Escalation triggers:** Any node-kind removal, fact shape change, schema/capability drift, non-Razor regression, or parser ABI change expands verification to the full contract and release tiers.

**Assigned verification failure:** Workers stop and report when assigned verification fails unless this plan explicitly changes that gate.

**Verification ledger:** Record invariant, command, scope label, grammar SHA, Julie commit SHA, result, and timestamp. Reuse evidence only at the same pair of SHAs.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Correct the expression node contract | None - serial | `crates/julie-extractors/src/razor/mod.rs`, `crates/julie-extractors/src/base/framework_structural_facts/razor.rs`, focused Razor tests | Yes | Establishes a committed green extractor fix before the parser pin changes. |
| Task 2: Pin the hardened grammar and add current-syntax gates | None - serial | `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/tests/razor/current_syntax.rs`, `crates/julie-extractors/src/tests/razor/mod.rs`, `fixtures/extraction/capabilities.json`, current-syntax fixtures | Yes | Requires Task 1's recorded green SHA and the verified tree-sitter commit. |
| Task 3: Tighten existing facts and golden contracts | None - serial | `component_reference.rs`, `structural_facts.rs`, `task9_fixtures.rs`, relevant Razor golden/evidence files | Yes | Requires Task 2's parser pin and generated current-syntax goldens. |
| Task 4: Certify real corpora and document the result | None - serial | `docs/plans/2026-07-11-blazor-corpus-classification.md`, optional new certification evidence, plan checkboxes | Yes | Requires Task 3's semantic assertions and golden fixtures at a committed HEAD. |
| Task 5: Run branch gates and prepare the release handoff | None - serial | Plan progress and release-prep evidence only | Yes | Requires all code, fixtures, docs, and corpus evidence at one HEAD. |

### Task 1: Correct the expression node contract

**Files:**
- Modify: `crates/julie-extractors/src/razor/mod.rs:156-218`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/razor.rs:60-96`
- Modify: `crates/julie-extractors/src/tests/razor/mod.rs`
- Modify: `crates/julie-extractors/src/tests/razor/structural_facts.rs`

**Interfaces:**
- Consumes: Grammar node kinds `razor_explicit_expression` and `razor_implicit_expression`.
- Produces: One crate-private predicate used by both symbol and structural-fact traversal.

**Contract inputs:** There is no named `razor_expression` node in the pinned grammar.

**File ownership:** `crates/julie-extractors/src/razor/mod.rs`, `crates/julie-extractors/src/base/framework_structural_facts/razor.rs`, focused expression tests

**Serialization required:** Yes.

**Dependency reason:** Establishes a committed green extractor fix before the parser pin changes.

**Step 1: Write failing explicit-expression tests**

Add one symbol test and one structural-fact test through the public extractor path:

```rust
#[test]
fn explicit_razor_expression_emits_template_expression_fact() {
    let results = extract("Pages/Index.razor", "<p>@(Model.Title)</p>");
    assert!(results.parse_diagnostics.is_empty(), "{:#?}", results.parse_diagnostics);
    assert!(facts_with_pattern(&results, "razor.template_expression.v1")
        .iter()
        .any(|fact| fact.metadata.get("expression").is_some()));
}
```

Add the corresponding symbol assertion using the existing expression symbol vocabulary rather than inventing a new kind.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p julie-extractors explicit_razor_expression -- --nocapture`

Expected: FAIL because both visitors match `razor_expression` instead of `razor_explicit_expression`.

**Step 3: Implement one private predicate**

Add a crate-private helper in `razor/mod.rs` and use it from both visitors:

```rust
pub(crate) fn is_razor_expression_node_kind(kind: &str) -> bool {
    matches!(kind, "razor_explicit_expression" | "razor_implicit_expression")
}
```

Replace both stale string-match arms with predicate guards. Do not expose this helper outside the crate or duplicate the node list.

**Step 4: Run focused and Razor language tests**

Run: `cargo test -p julie-extractors explicit_razor_expression -- --nocapture` and the Razor language command reported by `cargo xtask test list`.

Expected: PASS with no changed fact IDs or metadata keys.

**Step 5: Apply commit mode**

Use `serial-worker-commit`; record the green extractor-fix SHA. Task 2 starts only after this commit is integrated, so its old-pin red gate runs against an intentional clean HEAD.

**Acceptance criteria:**
- [x] Both explicit and implicit node kinds traverse through one private predicate.
- [x] Explicit expressions produce existing symbol/fact behavior.
- [x] No public API or artifact schema is widened.

### Task 2: Pin the hardened grammar and add current-syntax gates

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml:55`
- Modify: `Cargo.lock`
- Create: `crates/julie-extractors/src/tests/razor/current_syntax.rs`
- Modify: `crates/julie-extractors/src/tests/razor/mod.rs`
- Modify: `fixtures/extraction/capabilities.json`
- Create: `fixtures/extraction/razor/current-syntax/source.razor`
- Create: `fixtures/extraction/razor/current-syntax/expected.json`
- Create: `fixtures/extraction/razor/current-syntax/evidence.json`

**Interfaces:**
- Consumes: Exact verified SHA produced by the tree-sitter plan.
- Produces: Reproducible parser dependency plus fast and golden current-syntax evidence.

**Contract inputs:** Capture the grammar SHA with `git -C /Users/murphy/source/tree-sitter-razor rev-parse HEAD`; require a clean grammar worktree and its recorded passing branch gate before editing Cargo files.

**File ownership:** Parser manifests, `current_syntax.rs`, its module declaration, the Razor fixture registration in `fixtures/extraction/capabilities.json`, and `fixtures/extraction/razor/current-syntax/**`

**Serialization required:** Yes.

**Dependency reason:** Requires Task 1's recorded green SHA and the verified tree-sitter commit.

**Step 1: Write the current-syntax test before changing the pin**

Use a table-driven test with exact valid snippets:

```rust
let cases = [
    ("doctype", "<!DOCTYPE html><html><body></body></html>"),
    ("qualified component", "<BlazorSample.AdminComponents.Pages.ProductDetail />"),
    ("void and unquoted", "<head><base href=/CoolApp/></head><input disabled>"),
    ("entities", "<p>Tom &amp; Jerry &#x1F63A;</p>"),
    ("single quoted", "<input class='form-control'>"),
    ("nested block", "<div>@{ var value = 1; }<span>@value</span></div>"),
    ("bare page", "@page\n<h1>Page</h1>"),
    ("template", "@{ Func<dynamic, object> t = @<p>@item.Name</p>; }"),
    ("escape", "<p>@@ @(DateTime.Now).</p>"),
    ("tag helper", "@addTagHelper My.TagHelpers.EmailTagHelper, My.Assembly"),
];
for (name, source) in cases {
    let results = extract(name, source);
    assert!(results.parse_diagnostics.is_empty(), "{name}: {:#?}", results.parse_diagnostics);
}
```

Add one malformed-quote case that expects a diagnostic but proves a following component/route fact remains extractable.

**Step 2: Run tests on the old pin**

Run: `cargo test -p julie-extractors current_razor_syntax -- --nocapture`

Expected: FAIL for documented current syntax with Task 1 already committed and the old grammar pin still active.

**Step 3: Pin the exact grammar commit**

Read the SHA from the completed grammar verification ledger, independently confirm it with git, replace the `rev` value in `crates/julie-extractors/Cargo.toml`, and run `cargo update -p tree-sitter-razor` to update `Cargo.lock`. Review the lock diff to confirm only the intended git source and transitive consequences changed.

**Step 4: Generate and review current-syntax goldens**

Register `current-syntax` in the Razor fixture list in `fixtures/extraction/capabilities.json`. After the new pin passes the focused current-syntax test, generate `expected.json` through the repository workflow:

```bash
UPDATE_GOLDEN=1 cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
```

Review every generated row. Author `evidence.json` from that reviewed canonical output as a small stable set of expected symbols/facts, and make `current_syntax.rs` assert those entries; the golden updater does not generate semantic evidence files. Confirm the malformed fixture retains its diagnostic while following valid facts survive. Then run the focused current-syntax test, Razor language tier, and `cargo xtask test golden` without `UPDATE_GOLDEN`.

Expected: PASS with empty diagnostics for every valid case, bounded recovery for the malformed case, and no unreviewed golden changes.

**Step 5: Apply commit mode**

Use `serial-worker-commit`; record both the grammar SHA and Julie commit SHA.

**Acceptance criteria:**
- [x] Cargo manifest and lockfile resolve the exact verified grammar commit.
- [x] Official-current valid cases have zero parse diagnostics.
- [x] Malformed input preserves following structural evidence without being mislabeled clean.
- [x] Golden fixture output reflects real extractor behavior, not hand-authored aspirations.

### Task 3: Tighten existing facts and golden contracts

**Files:**
- Modify: `crates/julie-extractors/src/tests/razor/component_reference.rs`
- Modify: `crates/julie-extractors/src/tests/razor/structural_facts.rs`
- Modify: `crates/julie-extractors/src/tests/razor/task9_fixtures.rs`
- Modify: affected `fixtures/extraction/razor/**/{expected,evidence,golden}.json` only when regenerated output proves a change

**Interfaces:**
- Consumes: Task 2 parser pin and existing fact patterns.
- Produces: Tests that require both semantic facts and clean parsing for valid syntax.

**Contract inputs:** Existing component, route, code-block, template-expression, and directive facts remain authoritative.

**File ownership:** Existing Razor fact tests and only their directly affected golden/evidence files.

**Serialization required:** Yes.

**Dependency reason:** Requires Task 2's parser pin and generated current-syntax goldens.

**Step 1: Strengthen failing tests**

At the beginning of fully-qualified component and unquoted-href/attribute tests, add:

```rust
assert!(
    results.parse_diagnostics.is_empty(),
    "expected clean Razor parse: {:#?}",
    results.parse_diagnostics
);
```

Add explicit assertions for dotted tag metadata, generic attributes, unquoted route targets, and the following sibling fact after recovery.

**Step 2: Run the strengthened tests**

Run the exact test names from `component_reference.rs` and `structural_facts.rs`.

Expected: PASS only with the Task 2 pin; any error-recovery-only behavior fails.

**Step 3: Regenerate golden output through project tooling**

Use the repository's existing golden update workflow. Review each JSON change; update capabilities only when the observed support claim changes. Do not add a pattern ID, registry row, or exported contract entry.

**Step 4: Run golden, capability, and contract tiers**

Run: `cargo xtask test golden`, `cargo xtask test capability`, and `cargo xtask test contract`.

Expected: PASS; existing structural-fact IDs and shapes remain stable.

**Step 5: Apply commit mode**

Use `serial-worker-commit`; record the semantic/golden SHA before corpus certification starts.

**Acceptance criteria:**
- [x] Fully qualified tags and unquoted attributes require clean parses.
- [x] Existing facts retain IDs and metadata shapes.
- [x] No new fact family or workspace-global inference is added.

### Task 4: Certify real corpora and document the result

**Files:**
- Modify: `docs/plans/2026-07-11-blazor-corpus-classification.md`
- Create: `docs/release-evidence/2026-07-13-razor-parser-hardening.md`
- Modify: this plan's Task 4 checkboxes

**Interfaces:**
- Consumes: Task 2 parser pin and Task 3 semantic assertions/golden fixtures at a committed HEAD.
- Produces: Reproducible current-doc and Terraform corpus evidence.

**Contract inputs:** Prior release evidence processed 69/69 Terraform Razor files with zero failed parses/diagnostics. Recount the live corpus as `N`, require `N/N`, and investigate/report any drift from 69 rather than treating the historical number as the gate.

**File ownership:** Razor classification and new certification evidence only.

**Serialization required:** Yes.

**Dependency reason:** Requires Task 3's semantic assertions and golden fixtures at a committed HEAD.

**Step 1: Add current-doc corpus inputs**

Use checked-in fixtures or an explicitly named local corpus profile containing the official [.NET 10 Razor syntax](https://learn.microsoft.com/en-us/aspnet/core/mvc/views/razor?view=aspnetcore-10.0) and [Razor component](https://learn.microsoft.com/en-us/aspnet/core/blazor/components/?view=aspnetcore-10.0) examples. At execution time, record the current `dotnet/AspNetCore.Docs` commit and include every Razor-fenced example from `aspnetcore/release-notes/aspnetcore-11/includes/blazor.md`, retaining named coverage for `DisplayName`, `BasePath`, `NavLink RelativeToCurrentUri`, `EnvironmentBoundary`, MathML, and asynchronous form validation. Record URLs, retrieval date, source commit, and whether each input is stable or preview.

**Step 2: Run certification**

Run: `cargo xtask test certification` and the relevant real-world smoke profile.

Expected: PASS with zero unexpected Razor diagnostics.

**Step 3: Build the real CLI and dogfood Terraform**

Run:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
cargo xtask dogfood repo --root /Users/murphy/source/Terraform --out-dir target/dogfood/terraform-razor-hardening --binary target/release/julie-extract
```

Expected hard evidence: `N/N` live Razor files processed, zero failed parses, zero Razor diagnostics, valid artifact/report/JSONL outputs, and immediate rescan `no_change`; the evidence note records `N` and explains any drift from the prior 69.

**Step 4: Run strict quality reporting**

Run: `node scripts/language-data-quality-report.mjs --strict`.

Expected: `silent_cells=0` and `quality_bar_debts=0`.

**Step 5: Apply commit mode**

Use `serial-worker-commit`; record commands, SHAs, counts, outputs, and the certification SHA without release claims.

**Acceptance criteria:**
- [x] Stable and preview documentation inputs are labeled precisely.
- [x] Live Terraform counts are measured, not copied from prior evidence.
- [x] Strict quality debt remains zero.
- [x] No slow corpus leaks into the default tier.

### Task 5: Run branch gates and prepare the release handoff

**Files:**
- Modify: this plan's progress checkboxes
- Modify: release-prep documentation only if the user separately authorizes a release version

**Interfaces:**
- Consumes: Tasks 1-4 at a single clean HEAD.
- Produces: An implementation-complete, release-ready branch with exact dependency and evidence SHAs.

**Contract inputs:** Release publication, version choice, push, and tag remain explicit approval boundaries.

**File ownership:** Plan progress and approved release-prep docs only.

**Serialization required:** Yes.

**Dependency reason:** Requires all code, fixtures, docs, and corpus evidence at one HEAD.

**Step 1: Run changed-path and branch gates**

Run every command in the Branch gate, ending with `cargo deny check` and `cargo xtask release package-list`.

Expected: PASS at one HEAD.

**Step 2: Verify dependency and artifact contracts**

Confirm Cargo resolves the recorded grammar SHA, the parser inventory reports the expected ABI/language version, and SQLite/JSONL pattern contracts are unchanged.

**Step 3: Verify all worktrees**

Run root, branch, HEAD, status, and worktree inventory checks for Julie Extractors and the grammar worktree. Inspect every related worktree status and preserve unrelated changes.

**Step 4: Apply commit mode**

Use `serial-worker-commit`; record the final Julie SHA and verification ledger.

**Step 5: Stop at the approval boundary**

Report release readiness and the smallest remaining user decision: whether to push and publish a specific version. Do not choose a version, push, tag, or publish in this plan run.

**Verification ledger — 2026-07-13T21:39:38Z:**

- Final tested Julie Extractors HEAD: `480e0a6d50d20dc0175aca82e6b87d269d3ace81`; the implementation and replay fixtures were tested at `37d6941909ba4d31f5979533002019e5bf19212c`, followed only by final evidence-document corrections.
- Final certified parser content after review remediation: `fba8571f06c06aa5acca01e3d762f5a5e78dc50f`; certification document: `9fdcfd755d5537e8285166c25c34d1617bdf0826`; parser ABI: `15`.
- `cargo xtask test changed crates/julie-extractors/Cargo.toml crates/julie-extractors/src/razor/mod.rs crates/julie-extractors/src/base/framework_structural_facts/razor.rs fixtures/extraction/razor`: PASS.
- `cargo xtask test default`: PASS; extractor `2872` passed and `7` ignored, with artifact and CLI suites green.
- `cargo xtask test language razor`: PASS, `97/97`.
- `cargo xtask test golden`: PASS, `3/3`.
- `cargo xtask test capability`: PASS, `40/40` across capability and pending-shape contracts.
- `cargo xtask test contract`: PASS, including downstream path-dependency, SQLite schema/report, JSONL, CLI, path-policy, and operations contracts.
- `cargo xtask test certification`: PASS, `42/42` across capability, pending-shape, and parser-upgrade gates.
- `node scripts/language-data-quality-report.mjs --strict`: PASS; `silent_cells=0`, `quality_bar_debts=0`.
- Task 4 evidence was read back from the final artifacts: Terraform commit `821e6b1a268cb392b1abb5080243a299db2a9bc9`, Razor `28/28`, zero failed parses, zero Razor diagnostics, SQLite integrity `ok`, `103079` valid JSONL records, and immediate rescan `no_change`. The replayable documentation corpus contains `85` inputs with `14` classified placeholder/pseudocode diagnostics and no unexpected diagnostic on a valid example.
- `cargo deny check`: PASS; advisories, bans, licenses, and sources are green, with existing duplicate and wildcard warnings only.
- `cargo xtask release package-list`: PASS.
- Cargo resolves `tree-sitter-razor` to `fba8571f`; generated parser inventory reports language version/ABI `15` and only the public `razor_explicit_expression` and `razor_implicit_expression` expression nodes.
- No artifact implementation or contract documentation changed from `0d734ae`; the contract gate reconfirmed SQLite schema v4, JSONL v3, report, CLI, and operations compatibility.
- All Julie Extractors and tree-sitter-razor worktrees were inventoried by path, branch, HEAD, and status. Unrelated primary untracked plans, parser index files, and the pre-existing dirty `/private/tmp/razor-base` state remain untouched.
- No version was chosen and no push, tag, publish, or release action was executed.

**Acceptance criteria:**
- [x] All branch gates pass at one clean task HEAD.
- [x] Grammar and Julie SHAs are recorded together.
- [x] Existing artifact contracts remain compatible.
- [x] Release action remains unexecuted pending explicit approval.
