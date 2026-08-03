# 2026-08-03 — Cross-model review of the fleet-safety scan flags

**Scope:** `173ae45..76320df` — the `--spool-dir`, `--progress-file`, and `--parent-pid` work
(19 files, +4433/-96, plus the Windows identity fix), reviewed adversarially and independently by
**Codex** (gpt-5.1-codex-max) and **Grok** (grok-4.5), read-only, structured output. Both were given
the same focus: these three primitives all DELETE or KILL, and a wrong decision is unrecoverable.

**Result: 12 findings raised (7 Codex, 5 Grok), 4 distinct issues confirmed and fixed, 1 refuted
against the real consumer, the rest recorded.** Convergence was again the signal that mattered: the
three issues both models found independently are exactly the three that were real.

---

## Confirmed and fixed

**1. The progress file was truncated before identity was proven.**
*(codex critical 0.99, grok critical 0.90 — independent.)*
`create_for_artifact` ran the path guard, then `File::create` — which truncates as it opens — then
ran the guard again. The recheck can only report damage already done, so anything that re-points the
path between the guard and the create destroys a multi-gigabyte artifact with the guard's blessing.
Third round of this bug class and the first one that removes the window instead of narrowing it: the
file is opened without truncating, `same_file::Handle::from_file` proves THAT handle is not the
artifact, and `set_len(0)` runs only afterwards. Decision 8b.

**2. The reaper released the sentinel lock before unlinking the spool.**
*(codex high 0.97, grok high 0.88 — independent, same recommendation.)*
The unlocked-and-still-present window is not reachable today, because sentinel names embed a pid and
a nanosecond stamp so no starting scan adopts an existing one. That is a naming scheme guarding a
deletion, and the reason this flag reaps by lock at all is that a naming scheme is not evidence of
liveness. The claim is now held through the unlink, matching what the scan's own retirement always
did. Decision 11a.

**3. `cli.md` contradicted itself on Windows progress identity.** *(grok medium 0.95.)*
The Path Rules section had been updated to say identity is exact on every platform; the `scan`
section still said Windows falls back to a path comparison "where a hard link is therefore not
detected". Both are now the same statement. This was a miss in the same session's own fix, caught by
the reviewer.

**4. The Windows hard-link hole itself.** Not raised by either reviewer as a finding — Codex
explicitly noted the supplied diff's hole was already closed by the branch tip. Recorded here
because it is the same bug class as 1 and its fix (decision 8a, `same-file`) is what made 1's fix
one line of comparison rather than a platform split.

## Refuted

**5. "New report codes are added without a report schema bump."** *(codex high 0.98; grok raised the
same at 0.72 and reached the opposite, correct conclusion.)*
`ReportCode` is a closed Rust enum INSIDE this workspace; it serializes to a string, and the wire
format is what a consumer sees. Checked against the only consumer that exists: Miller parses
`ReportDiagnostic.code` as a plain `string` with no allow-list and no branching on specific codes
(`src/Miller.Indexing/ExtractReport.cs`). Unknown codes are inert to it. No bump is warranted, and
shipping one would be a compatibility event manufactured out of an internal type.

## Recorded, not acted on

| # | Finding | Why not acted on |
|---|---|---|
| 6 | `--parent-pid` is a silent no-op on Windows *(codex 0.99, grok 0.86)* | Real and documented (decision 5): `std` exposes no Windows `parent_id`. Both proposed remedies are worse — rejecting the flag breaks the "one caller argv works on every platform" rule that also governs `--progress-file` case handling, and a warning on every Windows scan is the permanently-firing warning decision 9 exists to avoid. The containment that actually works on Windows is a kill-on-close job object, which belongs to the supervisor; Miller's W6 wiring owns it. |
| 7 | A spool directory containing source is accepted and yields an incomplete artifact *(codex 0.99)* | Decision 9 chose warn-not-fail deliberately. Codex is right that the `spool_dir_excluded` trigger is already precise enough to fail on, but failing a scan because a stray file landed in a scratch directory trades a warning for an outage on a supervisor-facing flag. Miller, the consumer this was built for, points `--spool-dir` at a dedicated `.miller/spool` and cannot reach the case. |
| 8 | The watchdog cannot terminate a scan blocked inside one work unit *(codex 0.98)* | Accurate and by construction: the abort is cooperative because `process::exit` skips `Drop`, and `Drop` is the only thing that removes the spool — an exiting watchdog leaks precisely what `--spool-dir` exists to stop leaking (decision 5). A hard kill after a grace period is the supervisor's job and it already has one. |
| 9 | A permanently failing progress sink is silent *(codex 0.99)* | The contract is explicit that a mid-scan write failure never fails the scan, and a warning in the final report cannot help a supervisor that kills before the report exists. The mitigation is on the consumer side and already present: Miller's progress stamp SUMS the progress file's length with the artifact's bytes and the child's output lines, so a dead progress file degrades to the pre-2.22.0 signal rather than to no signal. |
| 10 | Node-local `flock` lets two hosts sharing one spool dir reap each other *(grok 0.88, codex 0.97)* | Documented accepted limit: give each machine its own spool directory. Neither "refuse on a network filesystem" nor "require a per-host namespace" is portably detectable from inside the extractor, and the minimum-age veto remains as a second guard. |

---

## Notes on the method

- **Convergence picked the winners again.** Three issues were found by both models independently;
  all three were real, and the two most severe findings on the branch were among them. Of the
  single-reviewer findings, one was actionable (the doc contradiction) and one was confidently wrong.
- **Confidence remains uncalibrated.** The refuted finding carried 0.98; the reviewer who was right
  about the same code carried 0.72. Read the number as how sure the model sounds.
- **The most valuable finding was the reviewer catching a fix's own leftovers.** The identity work
  had been done an hour earlier in the same session and left one contradictory paragraph behind.
