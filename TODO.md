# TODO

Lightweight tracker for open, agreed-but-not-yet-done work on the extraction
product. One section per item, each with a concrete file reference, why it
matters, and the proposed fix. No "later" placeholders.

Status legend: `open` (verified present), `partial` (partly done), `idea`
(proposed, not committed to).

---

## 1. `export --out -` failure arm writes the JSON report to stdout (F10/F29) — open

- **Where:** `crates/julie-extract-cli/src/commands.rs:792`
- **What:** The export success arm routes the report to `ReportStream::Stderr`
  when `--out` is `-` (`commands.rs:770-774`), but the failure arm at
  `commands.rs:792` is `outcome(report, 1, args.json, ReportStream::Stdout)` —
  unconditional stdout. When `--out -` has already streamed partial JSONL to
  stdout and the export then fails mid-stream, the failed JSON report lands on
  the **same stream** as the partial JSONL, corrupting the data channel a machine
  consumer is parsing.
- **Contract violated:** `docs/contracts/reports.md` ("`export --out - --json`
  writes JSONL to stdout and the final report to stderr") and `cli.md`
  ("JSONL uses stdout and the JSON report uses stderr").
- **Fix:** Make the failure arm mirror the success arm —
  `if args.out == Path::new("-") { ReportStream::Stderr } else { ReportStream::Stdout }`.
  Add an end-to-end test that triggers a mid-stream export failure with `--out -`
  (e.g. a malformed `metadata_json` row, as in `jsonl_contract.rs`) and asserts
  stdout holds only (partial) JSONL and the failed report is on stderr.
- **Effort:** small (1 line + 1 test). **Priority:** high for stdout-discipline.

## 2. No `cargo-deny` supply-chain / license / advisory gate — idea

- **Where:** repo root (no `deny.toml`); CI at `.github/workflows/ci.yml`.
- **What:** Nothing checks dependency advisories, license allow-list, or
  duplicate/banned crates. For a product whose contract is "spawn a binary and
  read SQLite/JSONL", a known-bad transitive dep would ship silently.
- **Fix:** Add `deny.toml` (advisories + licenses + bans) and a CI step
  (`cargo deny check`). Decide the license allow-list and whether advisories are
  deny vs warn. Pairs with item 3.
- **Effort:** medium. **Priority:** medium (no production consumer yet).

## 3. Migrate the standalone `md5` 0.7 dep to RustCrypto `md-5` — open (low)

- **Where:** `crates/julie-extractors/Cargo.toml:74` (`md5 = "0.7"`); usages in
  `crates/julie-extractors/src/tests/{jsonl_pipeline,path_identity}.rs`,
  `src/tests/vue/mod.rs`, `src/tests/html/script_style.rs` (all `md5::compute`,
  test-only helpers that recompute expected ids).
- **What:** `md5` 0.7 is a thin standalone crate; `md-5` (RustCrypto) is the
  actively maintained, audited equivalent and the conventional choice. This is
  exactly the kind of dep `cargo-deny` advisories/bans would surface (item 2).
- **Caveat to confirm first:** a source grep finds `md5::compute` only under
  `src/tests/`, but confirm no **non-test** id path uses it before swapping. If
  id generation is ever md5-based, a crate swap must keep digests byte-identical
  (both crates produce the same MD5) — characterize with the existing id-stability
  tests first.
- **Effort:** small. **Priority:** low (test-only today; do it with item 2).

## 4. Legacy `julie-extractors` clippy warnings not gated (residual of F20) — partial

- **Where:** `.github/workflows/ci.yml:27-33` gates only
  `julie-extract-artifact`, `julie-extract-cli`, `xtask` (`--lib --bins -D warnings`).
- **What:** F20's core recommendation (add a clippy gate, production-only
  enforcement) is **done** for the three product crates. The legacy
  `julie-extractors` lib still emits ~485 warnings (mostly test-code idioms like
  `assert!(x.len() > 0)`), deliberately outside CI scope.
- **Fix (if desired):** one-time `cargo clippy --fix` sweep of `julie-extractors`,
  then either add it to the gated `-p` set or document the intentional exclusion.
- **Effort:** medium (autofix is large but mechanical). **Priority:** low.
