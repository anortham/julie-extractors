# TODO

Lightweight tracker for open, agreed-but-not-yet-done work on the extraction
product. One section per item, each with a concrete file reference, why it
matters, and the proposed fix. No "later" placeholders.

Status legend: `open` (verified present), `partial` (partly done), `idea`
(proposed, not committed to).

---

## 1. No `cargo-deny` supply-chain / license / advisory gate — idea

- **Where:** repo root (no `deny.toml`); CI at `.github/workflows/ci.yml`.
- **What:** Nothing checks dependency advisories, license allow-list, or
  duplicate/banned crates. For a product whose contract is "spawn a binary and
  read SQLite/JSONL", a known-bad transitive dep would ship silently.
- **Fix:** Add `deny.toml` (advisories + licenses + bans) and a CI step
  (`cargo deny check`). Decide the license allow-list and whether advisories are
  deny vs warn. Pairs with item 2.
- **Effort:** medium. **Priority:** medium (no production consumer yet).

## 2. Evaluate migrating standalone `md5` 0.7 to RustCrypto `md-5` — open

- **Where:** `crates/julie-extractors/Cargo.toml:74` (`md5 = "0.7"`); usages in
  production ID/hash paths:
  `crates/julie-extractors/src/base/{extractor.rs,types.rs,body.rs,results_normalization.rs}`;
  expected-value helpers also use it under `crates/julie-extractors/src/tests/`.
- **What:** This is not test-only. The legacy extractor crate uses MD5-derived
  stable IDs and body digests in production output. Any crate swap must prove
  byte-identical digests and preserve existing ID-stability fixtures.
- **Fix:** Add supply-chain policy first (item 1), then either keep the crate as
  an explicitly allowed compatibility dependency or migrate to `md-5` with
  targeted stability tests for the production ID/hash paths.
- **Effort:** medium. **Priority:** medium because this touches output identity.

## 3. Legacy `julie-extractors` clippy warnings not gated (residual of F20) — partial

- **Where:** `.github/workflows/ci.yml:27-33` gates only
  `julie-extract-artifact`, `julie-extract-cli`, `xtask` (`--lib --bins -D warnings`).
- **What:** F20's core recommendation (add a clippy gate, production-only
  enforcement) is **done** for the three product crates. The legacy
  `julie-extractors` crate still emits warnings in production extractor modules
  and tests, deliberately outside CI scope.
- **Fix (if desired):** one-time `cargo clippy --fix` sweep of `julie-extractors`,
  then either add it to the gated `-p` set or document the intentional exclusion.
- **Effort:** medium (autofix is large but mechanical). **Priority:** low.
