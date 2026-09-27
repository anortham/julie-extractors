# Extraction data correctness

**Goal:** Preserve reference occurrences and prevent artifacts mixing extraction generations.
**Spec:** The user's request to fix the two reproduced data correctness issues; acceptance criteria below are the executable scope.
**Architecture:** Keep occurrence identity separate from exact target coordinates. Reuse producer versions and fingerprints to guard writes; retain SQLite's transaction boundary for a complete generation change.
**Architecture Quality:** No new service, resolver, or schema table. The main risks are false reference deduplication and retaining old facts while stamping new producer metadata.
**Authority:** Implementation and local commits authorized by the user. Push, publication, and release are not authorized. External reviewer: none. Astra reviewed the plan's data integrity choices before implementation.

## Global constraints

- Work only in `/home/murphy/source/julie-extractors/.worktrees/fix-data-correctness`, branch `fix/data-correctness`; code-kb is owned by another session.
- Apply shared fixes across applicable languages. Historical docs do not prevent correcting bad behavior.
- Never publish context spans as exact target tokens. Never invent occurrence identity from sequence numbers when source positions are available.
- Preserve exact identifier/reference-site joins and deduplicate repeated evidence of the same occurrence.
- Keep read compatibility; stale incremental writes must fail before mutation with actionable force-scan recovery.
- A generation-changing force scan must replace every retained fact and producer metadata together, or preserve the previous artifact on errors.
- No new dependencies. No narrated inline comments or test comments.

## Verification strategy

Project source: `AGENTS.md`, Cargo manifests, and xtask test tiers.
Worker red/green: focused Rust tests through extraction/CLI/artifact interfaces. Record commands, failing assertions, passing output, and logs.
Worker ceiling: only new regression tests plus directly affected existing test modules. Lead owns broader tiers.
Lead affected-change: CLI contracts, artifact writer/schema contracts, reference-site identity contracts, and changed language tests.
Branch gate: `cargo xtask test default`, formatting and diff checks. Run golden/capability gates if extraction output or fixtures change.
Security scope: none declared; no dependency or security surface changes.
Specialist: Windows tests for write/rebuild behavior via win-test; no real-world corpus sweep unless a failure requires it.
Hard assertions: row multiplicity, stable identity, site precision, refusal before mutation, atomic force replacement, empty scan success.
Workers may fix failures in owned scope; report conflicts or required ownership expansion. Lead records verification and does not repeat passing unchanged scopes.

## Parallel execution contract

Commit mode: `parallel-lead-commit`. Workers never stage or commit and must not revert each other's edits. Lead implemented Task 2 after the emitter audit, and owns artifact_access.rs/capability_snapshot.rs within Task 1; the worker retains commands.rs, reports.rs, writer.rs and freshness integration tests.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| 1: Producer freshness | A | CLI artifact_access.rs, commands.rs, capability_snapshot.rs; artifact writer.rs and writer submodules if necessary; CLI producer_freshness.rs tests; artifact writer tests as needed | No | Independent of reference mapping |
| 2: Reference occurrences | A | CLI extraction.rs and reference_occurrences.rs tests; extractor base and language implementation/tests as required, excluding lib.rs | No | Independent of producer guards |
| 3: Integration | B | Extractor lib.rs semantic version, contract docs, this plan, memories, fixture expectations if required | Yes | Review and integrate both fixes before broad verification |

### Task 1: Producer freshness

**Interfaces:** `ArtifactMetadata`, `open_artifact`, `open_artifact_for_root`, `current_capability_fingerprints`, `scan_collecting_warnings`, artifact write transactions.
**Contract inputs:** Existing `FingerprintMismatch` error; `scan --force` recovery; current binary version, parser fingerprint, capability fingerprint, and `julie_extractors::EXTRACTION_CONTRACT_VERSION`.
**File ownership:** Task 1 row above. **Serialization required:** No. **Dependency reason:** Independent batch A.
**What to build:** Version the capability fingerprint payload and include the existing semantic extraction contract string. Compare all three producer values on incremental writes, including scan/update/delete/rebind. Reuse shared checks and keep schema error precedence and read compatibility.
**Approach:** Force must bypass only freshness, retain valid schema/root/level checks, and rewrite compatible artifacts transactionally. Never route a producer mismatch into generic unlink/recreate recovery. On generation change, discovery/read/extraction errors or interruption must leave prior facts and metadata untouched. Handle empty artifacts and failed writes. Assess transaction-time validation so concurrent producer changes cannot evade the early guard.

- [x] Focused regression fails before the fix and passes afterward.
- [x] Each producer mismatch, including old semantic fingerprint, prevents every incremental mutation; reads still work.
- [x] Unchanged files are re-extracted by successful force; removed files are removed; same-generation incremental behavior remains unchanged.
- [x] Generation-changing force errors and write failures preserve old rows and metadata; empty workspace can adopt the current generation.
- [x] Semantic contract alone changes the fingerprint; canonical row order remains stable.

### Task 2: Reference occurrences

**Interfaces:** `map_results`, `map_relationships`, `map_structured_pending`, `pending_id`, shared relationship construction/normalization, language emitters.
**Contract inputs:** Context spans are valid for occurrence identity but not exact reference-site coordinates; truly spanless duplicate emissions must not be assigned invented ordinal identity.
**File ownership:** Task 2 row above. **Serialization required:** No. **Dependency reason:** Independent batch A.
**What to build:** Preserve two same-line external calls and two same-line resolved calls in the SQLite output using existing spans for identity. Check upstream normalization and any relevant spanless emitter loss; add context at emission where needed. Keep published nonexact site coordinates null and exact site joins intact.
**Approach:** Start with real source-to-artifact regressions at facts and full levels. Verify dedup of identical evidence and deterministic IDs. Cover several language families and audited exact emitters; do not make Rust a special case. Inspect truly spanless relationship emitters and fix reproduced occurrence loss at the source.

- [x] Focused public-interface regressions fail before the fix and pass afterward.
- [x] Distinct pending and resolved same-line occurrences survive; identical occurrence evidence still deduplicates.
- [x] Exact-site coordinates and identifier joins are unchanged; broad spans stay nonexact.
- [x] Stable IDs and applicable language coverage are verified, including fixes to any reproduced upstream loss.

### Task 3: Integration and contracts

**Interfaces:** Extraction semantic version and public contract documentation.
**File ownership:** Task 3 row above. **Serialization required:** Yes. **Dependency reason:** Both fixes must exist before broad verification.

- [x] Review each implementation against live source, tests, and the criteria above; resolve defects.
- [x] Advance semantic output identity and document freshness/occurrence behavior without changing the schema unnecessarily.
- [ ] Run relevant Linux and Windows verification, update any intentionally affected goldens, and commit reviewed work locally.
- [ ] Reconcile all worktree states and report the result and recovery command concisely.
