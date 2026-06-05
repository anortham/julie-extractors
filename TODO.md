# TODO

Lightweight tracker for open, agreed-but-not-yet-done work on the extraction
product. One section per item, each with a concrete file reference, why it
matters, and the proposed fix. No "later" placeholders.

Status legend: `open` (verified present), `partial` (partly done), `idea`
(proposed, not committed to), `done` (implemented and verified).

---

## 1. No `cargo-deny` supply-chain / license / advisory gate — done

- **Where:** `deny.toml`; `.github/workflows/ci.yml`.
- **What changed:** Added a cargo-deny policy covering advisories, SPDX license
  allow-listing, duplicate/wildcard warnings, exact git-source allow-listing,
  and an explicit compatibility decision for `md5@0.7.0`.
- **Verification:** `cargo deny check` passes locally. Duplicate versions and
  path/git wildcard requirements are warnings; advisories, license policy, and
  unknown dependency sources are hard gates. CI runs
  `EmbarkStudios/cargo-deny-action@v2` with `--all-features`.

## 2. Evaluate migrating standalone `md5` 0.7 to RustCrypto `md-5` — done

- **Where:** `crates/julie-extractors/Cargo.toml:74` (`md5 = "0.7"`); usages in
  production ID/hash paths:
  `crates/julie-extractors/src/base/{extractor.rs,types.rs,body.rs,results_normalization.rs}`;
  expected-value helpers also use it under `crates/julie-extractors/src/tests/`.
- **Decision:** Keep `md5@0.7.0` as an explicitly allowed compatibility
  dependency. The MD5 output is part of stable legacy extraction IDs and body
  hashes, so changing crates would be an artifact identity migration rather than
  a supply-chain cleanup.
- **Verification:** `deny.toml` explicitly allows only `md5@0.7.0` with the
  compatibility reason above; strict workspace clippy and the path-identity /
  contract gates pass locally.

## 3. Legacy `julie-extractors` clippy warnings not gated (residual of F20) — done

- **Where:** `.github/workflows/ci.yml`.
- **What changed:** CI now runs
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  covering the core extractor crate and test targets.
- **Verification:** strict workspace clippy passes locally.

## 4. Execute the 2026-06-05 project hardening review plan — done

- **Where:** `docs/plans/2026-06-05-project-hardening-review.md`.
- **What changed:** F1-F19 are implemented and marked fixed in the plan.
  Regression coverage was added for artifact correctness, JSONL atomicity,
  parser provenance, language detection, release preflight, default-suite
  tripwires, and workflow guardrails.
- **Verification:** `cargo xtask test default`, `cargo xtask test contract`,
  `cargo test -p xtask`, and strict workspace clippy pass locally.

## 5. TypeScript generic client-call URL literals are missing — done

- **Where:** `crates/julie-extractors/src/typescript/identifiers.rs`;
  `crates/julie-extractors/src/tests/typescript/literals.rs`;
  `crates/julie-extract-cli/tests/operations_contract.rs`.
- **Root cause:** For `await axios.get<T>(...)`, tree-sitter TypeScript reports
  the call callee through an awaited node whose fallback text was
  `await axios.get`. The literal was captured, but carrier classification only
  recognizes `axios.get`, so the language-policy pass dropped the row before it
  reached SQLite/JSONL.
- **What changed:** TypeScript callee carrier normalization now strips the
  `await ` prefix before carrier classification. Regression coverage verifies
  raw extractor output and the real CLI SQLite `literals` table for generic
  `axios.get<T>` and `axios.put<T>` calls.
- **Verification:** Focused raw TypeScript literal test passes; focused CLI
  operations contract passes; a MyraNext services smoke scan now persists
  `/api/messages/active` with `kind = url`, `carrier = axios.get`, and
  containing symbol `getActiveMessages`.
