# Task 7: Razor HTTP Client Requests

## RED

- Added five caller-facing canonical-pipeline tests in `tests/razor/client_request.rs` before production edits.
- `cargo test -p julie-extractors --offline razor::client_request -- --nocapture` produced the expected RED: the `@code` and `@functions` positives emitted zero facts; the three pre-existing-safety tests passed.

## GREEN

- `http.client_request.v1` now emits from proven `HttpClient` calls wholly contained by structured Razor `@code` and `@functions` blocks.
- Receiver proof is limited to structured `razor_inject_directive` and allowed embedded-C# ranges. Markup text cannot attest a receiver.
- The existing C# scanner accepts allowed call and receiver byte ranges. Its normal `.cs` entry point still supplies one full-file range, preserving its behavior and preventing duplicate dispatch.
- A same-length masked view isolates C# string/comment scanning to allowed ranges while all parsing and emitted spans retain absolute offsets into the original Razor source.
- Razor's emitted-pattern inventory, registry language list, and generated JSON contract now include `http.client_request.v1`.

## Miller Evidence

- Oriented with `context` in workspace `facd497e3541` on the C# HTTP collector, Razor framework dispatch, registry, and tests.
- Inspected `collect_csharp_http_client_requests`, `collect_method_calls`, and `collect_framework_structural_facts`; pre-change impact identified the backend HTTP dispatch and framework collector as the direct blast radius.
- Post-change workspace refresh revision 21 and git-diff impact confirmed the canonical framework dispatch, emitted-pattern inventory, registry serialization, and focused HTTP/Razor tests as the relevant verification surface.
- Inspected the final `collect_razor_http_client_requests` call graph after refresh.

## Architecture Quality

**Affected modules:** shared C# HTTP-client scanning, Razor framework-fact dispatch, structural-fact registry/export, and Razor contract tests.

**Caller-facing interface:** the existing canonical extraction result gains an already-versioned fact family for Razor; no new public Rust API is exposed.

**Depth/locality check:** Razor owns parser-backed range discovery. The established C# collector owns HttpClient method recognition and metadata. The registry remains the single source for the exported contract.

**Test surface:** all behavior is exercised through `pipeline::extract_canonical`, including exact source-byte slicing.

**Rejected shortcuts:** no whole-file Razor text scan, no markup receiver proof, no substring search for `@code`, no copied HTTP method table, and no offset rebasing.

**Architecture risk:** low. The range seam is private, C# keeps the full-file path, and all Razor gating is derived from named parser nodes.

## Verification

- Focused Task 7 tests: 5 passed, 0 failed.
- Backend HTTP regressions: 65 passed, 0 failed.
- Razor scope: 78 passed, 0 failed.
- Registry update/export and ungated sync: 9 passed, 0 failed.
- Registry emitted-pattern union with `test-capability-matrix`: 8 passed, 0 failed.
- Golden structural-fact conformance with `test-golden`: 1 passed, 0 failed.
- Package: 2,847 passed, 0 failed, 7 ignored; doctests 1 passed.
- `cargo clippy -p julie-extractors --offline --all-targets -- -D warnings -A clippy::collapsible_if`: passed. The allowance is for existing Task 5/6 `collapsible_if` findings outside Task 7 files.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

## Worktree Evidence

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`
- Branch: `codex/blazor-razor-support`
- Base commit before Task 7: `980a60c8d89abc22f777e8b3fd118be406c7d52d`
- Pre-commit dirty state contained only the Task 7 implementation, tests, generated contract JSON, and this report.
- The Task 7 commit contains this report; its hash is recorded in the handoff.
- No push was performed.
