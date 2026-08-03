# Fleet-safety scan flags: `--spool-dir`, `--progress-file`, `--parent-pid`

Date: 2026-08-02

## Context

A consumer ran several concurrent scans from git worktrees on one machine. The
machine went into the OOM killer; every killed scan left its extraction spool
behind, reaching roughly 130 GB over two days of retries; and the supervisor
killed healthy long scans because it could not see them making progress. The
supervisor owns the control plane and is fixing its own side. These three flags
are the parts only the extractor can provide.

All three are process-lifecycle concerns, not extraction concerns. No parser,
grammar, capability row, or language dispatch changes, so the language-parity
rule is satisfied by construction rather than by per-language verification.

## Decision 1 — every flag is opt-in with unchanged default behavior

An absent flag opens no file, starts no thread, and issues no extra syscall. The
artifact, the report, the exit code, and the filesystem side effects of a scan
without the flags are what they were before the flags existed. Contract tests
assert this against the real binary, not by inspection.

This is why spool locking and spool removal are both gated on `--spool-dir`
rather than always on. Locking is one syscall and removal is a directory read;
both are cheap, but "cheap" is not "none", and a supervisor that has not adopted
the flag must observe exactly what it observed before.

Sharing a `--spool-dir` with flagless scans is safe rather than forbidden — see
decision 2b: only a spool with a locked sentinel is ever a removal candidate, so
a flagless spool is never touched by anyone. It is also never cleaned up.

## Decision 2 — spool ownership is an advisory file lock, not a process-id probe

The original plan was to remove spool files whose embedded process id is provably
dead, with a rule that "access denied" means alive. That cannot be built here:
`unsafe_code = "forbid"` is set workspace-wide and every crate opts in, so
`libc::kill`, `OpenProcess`, `poll`, and kqueue are all unavailable, and `forbid`
cannot be relaxed in-crate. `rustix::process::test_kill_process` is safe at the
call site but Unix-only, and Windows is a shipped target.

The replacement is `std::fs::File::lock` / `try_lock`, verified empirically on
the installed toolchain under `#![forbid(unsafe_code)]`: a handle held by one
open file description makes another handle's `try_lock` return
`Err(WouldBlock)`, and `Ok(())` once released.

This is strictly better than the process-id design, not merely a workaround. The
kernel releases the lock on process death however the process dies, including
SIGKILL, so there is no process-id-reuse window at all, and it works on Windows.

The process id stays in the spool file name as a diagnostic. It is never the
removal authority. Age is never the removal authority either: candidates younger
than a short window are skipped because the lock target is created and then
locked as two adjacent operations, and a candidate inside that window may belong
to an owner that has not reached its lock yet. The filter only ever makes removal
more conservative.

## Decision 2a — the lock lives on a sentinel, never on the spool

The first implementation took the lock on a second handle to the spool file
itself. That is correct on macOS and Linux, where `File::lock` is `flock` and
advisory, and broken on Windows, where it is
`LockFileEx(.., LOCKFILE_EXCLUSIVE_LOCK, .., u32::MAX, u32::MAX)` over the whole
range and mandatory: the spool's own `BufWriter` handle and its later read handle
would both fail with a lock violation, so every `--spool-dir` scan on Windows
would fail with no artifact produced. CI is ubuntu-only, so it would have shipped
green and broken exactly the fleet the flag exists to protect.

The lock therefore lives on a sibling `<spool>.lock` sentinel and never touches
the spool's byte range on any platform. The sentinel is created with `create_new`
and locked BEFORE the spool file is created, so a spool never exists whose
sentinel is not already locked, and the guard releases and removes the sentinel
last so no reaper can act on a pair mid-retirement.

## Decision 2b — candidacy is structural, not a matter of operator discipline

Reaping used to accept any spool-shaped name and treat "not locked" as "unowned".
That made a flagless hand-run scan, or any filesystem where locking returns
`ENOLCK`, produce a spool that a concurrent scan would delete out from under it
once its mtime went stale.

Two changes make candidacy imply lock ownership:

- Spools that own a locked sentinel get a distinct name
  (`julie-extract-scan-owned-spool-…`); everything else keeps
  `julie-extract-scan-spool-…`.
- Removal iterates `*.lock` sentinels, not spools. A spool with no sentinel is
  never reached, so it can never be removed.

When the sentinel cannot be locked at creation time the scan falls back to the
non-candidate name in the requested directory rather than failing with a typed
path error. The flag exists to make concurrent scans safer; refusing to run on a
scratch mount that cannot take a lock would trade a leak for an outage, and the
fallback spool is still removed by the scan's own `Drop`. The residual cost is
that a hard-killed scan on such a mount leaks its spool — which is exactly the
pre-flag status quo, never worse.

## Decision 2c — the spool directory is excluded from discovery

Spool files end in `.jsonl`, and `jsonl` is a supported extension. Nothing
stopped a caller pointing `--spool-dir` inside `--root`, so a spool a concurrent
scan still owned would be walked, detected as JSON, and extracted into the
artifact as if it were source. The resolved spool directory is skipped at the
walk level, exactly as the progress file is, so it does not even increment
`unsupported_files`; files matching the spool or sentinel name shape directly
inside it are skipped too, which covers `--spool-dir` pointed at the root itself.

## Decision 3 — the spool guard owns cleanup from creation

Cleanup used to hang off the aggregate returned by the extraction pass, which is
constructed after the last spool write. A spool write error returned before that
point and left the file behind. The lock and the removal now live on the guard
that is created with the spool itself, so every early return between creating the
spool and handing it upward removes it.

## Decision 4 — progress is append-only JSONL, not rename-or-rewrite

The consumer needs to detect advance, and it already samples file lengths.
Append-only makes length a sound advance signal with a one-line consumer change
and no parsing, and makes a torn write always the last line.

Write-temp-then-rename was rejected: the destination momentarily does not exist,
so a length sampler sees the file disappear, and rename over an existing
destination is unreliable on Windows. Fixed-width in-place rewrite was rejected:
the length never changes after the first write, which kills the consumer's
cheapest signal and forces it to read and hash contents on every poll.

The throttle ticks with a relaxed atomic add inside the parallel extraction
closure, not only in the serial per-chunk drain. The drain fires once per 512
files, so a chunk of large files would emit nothing for its whole duration —
exactly the pathological case the file exists for. The write itself is gated by a
compare-exchange on a due-time atomic, so exactly one worker per interval touches
the file. Discovery consults the clock once per 256 directory entries rather than
per entry.

Two consequences of append-only are written into the consumer contract rather
than left implicit. Length is monotonic WITHIN one scan only — the next scan
handed the same path truncates the file, and the named consumer reuses one
progress path per workspace, so a length decrease must be read as a fresh
baseline and as progress rather than as the stall the file exists to prevent.
And `write_all` advances the file offset by whatever it wrote before failing, so
a full disk can leave a half-written record mid-file; the sink remembers that and
opens the next record with a newline that closes the truncated line, keeping the
damage to one droppable line so every later record still parses.

A `--progress-file` equal to `--db` or one of its `-wal`/`-shm` sidecars is
refused at argument time. Both paths are canonicalized in the same shape and are
in scope together, and the progress file is opened with `File::create`, which
truncates — a script templating bug pointing both at the same variable would
otherwise destroy a multi-GB artifact before the scan validated it could run. No
guard is added for a progress file INSIDE `--spool-dir`: after decision 2b the
reaper only ever removes a sentinel-backed spool name, which a progress file is
not, so the restriction would buy nothing.

`artifact_write` is deliberately left uninstrumented. Threading a callback
through the artifact writer would change a public API of the artifact crate for a
phase the consumer can already watch through artifact file sizes, and a scan that
wedges strictly inside that phase should be caught by the stall window.

## Decision 5 — the watchdog aborts cooperatively and never calls `process::exit`

`--parent-pid` polls `std::os::unix::process::parent_id` on a plain thread — the
crate is synchronous plus rayon, with no async runtime. Asking the kernel who the
parent is now, rather than probing whether a recorded id is still alive,
eliminates process-id reuse rather than mitigating it: an orphan is reparented to
init, so a recycled id can never re-become our parent.

The trip sets a cooperative abort flag checked between extraction chunks and
before the artifact is opened. It must not call `std::process::exit`. That skips
`Drop` in every thread, and `Drop` is the only thing that removes the extraction
spool — an exiting watchdog would leak precisely the file `--spool-dir` exists to
stop leaking. There is no `panic = "abort"` profile, so unwinding is reachable and
the abort returns a normal report through the existing exit path.

The scan does not abort once the artifact write transaction has started: the
spool must stay on disk until the writer has read it back, and the write is
atomic anyway. The acceptance criterion is therefore "no scan running beyond the
watchdog interval plus the in-flight artifact write", not "beyond the watchdog
interval".

The last abort point sits immediately BEFORE the scan's first destructive step.
A `--force` scan whose existing artifact cannot be opened — schema drift under
`--strict-schema`, partial corruption, or a recorded `root_path` that differs
from `--root` — unlinks the artifact and its `-wal`/`-shm` sidecars before the
writer runs. Deciding the abort after that step would let a scan delete the live
artifact and then report `parent_exited`, which `docs/contracts/reports.md`
documents as leaving the artifact untouched. The ordering is enforced by
`abort_before_full_rebuild`, which owns both statements so the sequence cannot
drift apart again.

The trip publishes the observed parent id with a `Relaxed` store and the flag
itself with a `Release` store, read back with `Acquire`. Two `Relaxed` stores to
different locations have no ordering relationship, so on aarch64 — the primary
release target — a thread that observed the trip could have read the id atomic's
zero default and reported a nonsense parent in the diagnostic.

The stdout-pipe-closure half of the original design was dropped. Nothing is
written to stdout during a scan — the report is written once, after the scan has
fully returned — so there is no mid-scan write to fail, and polling the
descriptor for hangup requires `unsafe`. It also adds no coverage over the parent
check, since stdout may not be a pipe at all.

`std` exposes no Windows counterpart for `parent_id`, so the watchdog is
Unix-only. The flag is accepted and ignored elsewhere rather than being a parse
error, so one caller argv works on every platform; Windows orphan containment
belongs to the consumer's kill-on-close job object.

## Decision 6 — the progress file is skipped by the walk, not counted as unsupported

Excluding it the way the artifact database is excluded would still count it in
`unsupported_files`, and therefore in `counts.files_scanned`. A scan must not
report different counts purely because it was asked to report progress, so the
discovery walk skips the path outright. `select_file` reports it as hard excluded
for the same `DiscoveryExclusions`, which is what makes a `scan` walk and a
direct `select_file` call agree.

That exclusion is scoped to the scan that owns it and protects nothing else.
`update` builds its policy with `DiscoveryExclusions::default()` and does not
accept `--progress-file` at all, so it has never seen the exclusion. What keeps
`update` off a progress file is decision 7's extension rule: `.progress` is not a
supported source extension, so `update --file scan.progress` is refused as an
unsupported file.

## Decision 7 — `--progress-file` must be named `.progress` or end in `.progress`

Creating the progress file truncates whatever is already at the path, at argument
time, before the scan validates anything. The path is then excluded from
discovery, so `files_scanned` stays internally consistent and the report is `ok`.
`--progress-file $ROOT/src/lib.rs` — one templating slip against the wrong
variable — therefore empties a source file and reports success with no
diagnostic.

Requiring a name nothing else uses makes that whole class impossible instead of
guarding one instance at a time, and it costs one comparison at argument time.
The refusal uses the existing `path_error_outcome` / `invalid_path` shape, like
every other bad path.

Two spellings are accepted, and the contract docs and the refusal message state
both:

- any name whose extension is `progress` (`scan.progress`), and
- the bare dotfile `.progress`.

The bare dotfile needs its own arm because Rust reads a leading-dot-only name as
a stem with NO extension, so an extension-only check refused `/ws/.progress` —
the most obvious spelling of a hidden progress file — with a message saying it
must use the extension it plainly has.

The comparison ignores ASCII case, deliberately. The rule's whole job is that the
suffix belongs to nothing else, which a case variant does not weaken; and on a
case-insensitive volume (default APFS, NTFS) `scan.PROGRESS` and `scan.progress`
ARE one file, so refusing one spelling would make the same argv work on one
machine and fail on the next — the same "one caller argv works everywhere"
reasoning as decision 5's Windows `--parent-pid`.

The check runs on the RESOLVED path, not the spelling the caller passed, so a
`report.progress` symlink pointing at a source file is refused rather than
followed.

## Decision 8 — the collision check compares paths resolved the same way

`--db` fully canonicalizes an existing artifact, resolving symlinks in the final
component. `--progress-file` originally canonicalized only the parent and
re-joined the raw file name, and the collision check compared the two with `==`.
With `/ws/artifact.sqlite` a symlink to `/data/artifact.sqlite` — a normal layout
for a large index on its own volume — `--db /ws/artifact.sqlite
--progress-file /ws/artifact.sqlite` resolved to two different strings, the guard
passed, and `File::create` followed the symlink and truncated the multi-gigabyte
artifact. That is exactly the outcome the guard was added to prevent.

`--progress-file` now resolves its final component too when the path exists, so
the two flags are compared in the same spelling.

The check compares against the artifact path only. It once also reserved the
`-wal` and `-shm` sidecars, and those arms were unreachable: decision 7 runs
first and a sidecar name always ends in `-wal` or `-shm`, so it can never end in
`.progress`. Worse, the test that claimed to prove the sidecar arms passed on the
name rule for all three inputs, so nothing was proving them at all. The arms are
gone and the tests now name the mechanism each input actually hits — the name
rule for the artifact and both sidecars, the collision guard for the one input
that reaches it, an artifact itself named `*.progress`. The sidecars lose no
protection: the name rule refuses them unconditionally, whether or not an
artifact happens to be open at that path.

## Decision 9 — a spool directory that SWALLOWS content warns

The resolved spool directory is skipped at the walk level and deliberately not
counted in `unsupported_files` (decision 2c), and it is created when missing, so
no existence check can catch a caller who typed the wrong variable.
`--spool-dir $ROOT/src` therefore exits `0` with `status: ok`, zero warnings, and
an artifact missing every symbol under `src/`; an incremental rescan then drops
the previously indexed rows for that subtree as missing. The consumer has no
signal at all.

The exclusion stays — not excluding it corrupts the artifact, which is why
decision 2c exists — but it stops being silent: a `spool_dir_excluded` warning
names the excluded directory. Refusing outright was rejected because
`$ROOT/.spool` is a legitimate layout.

The trigger is what the directory HOLDS, not where it sits. The first
implementation warned on any spool directory inside the root that was not the
root itself, which fires on exactly the legitimate layout this decision names:
`$ROOT/.spool`, and `$ROOT/.miller/spool`, the natural per-workspace scratch path
the consumer will wire up. Every scan would then carry a permanent warning that
no operator can act on, surfaced in the consumer's own health view — which is how
a warning channel stops being read, and it would have buried the `--spool-dir
$ROOT/src` case the warning exists for.

The signal is one non-recursive `read_dir` of the resolved spool directory: any
entry that is not a spool file or a sentinel means real content is being
excluded. A directory holding only this scan's leftovers is the flag working, not
content lost. An unreadable directory does not warn and does not fail the scan —
a missing signal is not evidence of a hazard, and a warning probe must never be
able to fail a scan.

A spool directory that IS the root is still not warned about: only spool-shaped
file names are skipped there, so no source is lost.

## Decision 9a — configuration warnings ride every exit, not just the success path

`spool_dir_excluded` and `spool_lock_unavailable` were attached inside the
write-success arm, so four exits that build their own report dropped them:
artifact-report failure, write failure, `ArtifactWriter::open_path` failure, and
the `parent_exited` abort. An operator who pointed `--spool-dir` at `$ROOT/src`
AND hit a write failure got a report with no `spool_dir_excluded` warning, fixed
the disk, reran, saw a clean `ok`, and never learned that the first run excluded
`src/` — the exact silence decision 9 exists to end, on the run most likely to be
read closely.

Both warnings describe how the scan was CONFIGURED rather than what it found, so
they are now collected as the scan resolves its arguments and attached once, by
the `scan` wrapper, to whatever report the run produced. Attaching them in one
place rather than at each `return` is also what keeps an early return added later
from silently dropping them again.

## Decision 10 — an unlockable spool directory warns

Decision 2b falls back to a non-candidate spool name when the sentinel cannot be
locked, rather than failing the scan. That trade is right — failing a scan on an
`ENOLCK` scratch mount trades a leak for an outage — but silently falling back
leaves an operator who adopted `--spool-dir` specifically to stop a 130 GB leak
with no way to learn the protection is inert. The fallback now emits a
`spool_lock_unavailable` warning, in the same spirit as `--parent-pid` being a
loud deterministic failure rather than a silent one.

## Decision 11 — the sentinel is removed only when its spool is gone

The sentinel is the ONLY thing that makes a spool a removal candidate, so
removing it while its spool survives converts a reapable leak into a permanent
one. Both the reaper and the scan's own retirement previously discarded the spool
removal result and removed the sentinel unconditionally, so one transient failure
was enough. Removal is now conditional on the spool being gone — removed, or
already absent. Releasing the lock is unconditional, so a kept pair is
immediately reapable by the next scan.

## Decision 12 — "stopped early" is carried, not re-derived

The extraction pass exits its chunk loop with a bare `break` and returns a
normally shaped result carrying a PARTIAL path set, and the caller independently
re-derived the abort condition from the same `AtomicBool`. They agreed only by
coincidence of both reading one monotonic flag. A second break condition added
later — an OOM/jobs-cap retry, a time budget, a memory-pressure check — without a
matching caller-side check would let the writer promote a partial spool as a
COMPLETE scan, deleting every file after the break point from the artifact.

The pass now returns a `SpoolCompletion` naming why it stopped, produced by the
same expression that decides to break, and the caller matches on it. A new stop
reason is a compile error at every caller instead of silent data loss.

## Note for the consumer — a workspace's own `.miller/` is walked today

`.miller` is not in `HARD_EXCLUDE_DIRS`, and `jsonl` is a supported source
extension. A workspace's own `.miller/logs/*.jsonl` are therefore walked and
extracted as JSON source unless an ignore file covers them, and the same is true
of any other `.jsonl` the consumer writes under its own state directory.

This is pre-existing and is NOT this workstream's to fix. Removing it would
change default discovery behavior for every caller, which this work promised not
to do; the right fix is the consumer shipping a `.julieignore` rule, or a
separate decision to add `.miller` to the hard-exclude set.

It bears directly on where the consumer puts its spool and progress paths. A
spool directory under `.miller/` is protected by decision 2c's walk-level
exclusion and by the spool-name skip regardless, and a `.progress` file is
excluded by decision 6, so neither flag is affected. But a consumer that assumes
`.miller/` as a whole is invisible to the extractor is wrong today.

## Decision 8a — identity is answered by `same-file`, so Windows is not a weaker case

Decision 8 compared resolved paths, which was the second of three rounds of the
same bug: two names for one file need not compare equal as text. Comparing
`dev`/`ino` closed it on Unix, but `std::os::windows::fs::MetadataExt`'s
`volume_serial_number` and `file_index` are behind the unstable
`windows_by_handle` feature, and `unsafe_code = "forbid"` in
`[workspace.lints.rust]` cannot be relaxed per crate, so the Win32 call behind
them was unreachable. Windows fell back to a case-insensitive path comparison,
which does not see an NTFS hard link — a documented hole through which
`--progress-file` truncates a multi-gigabyte artifact.

`same-file` answers identity on both platforms and is ALREADY in the lock graph
(`ignore` → `walkdir` → `same-file`), so taking it as a direct dependency of
`julie-extract-cli` adds no build unit, no new registry source, and no new
license — the reason to accept a documented hole instead of a dependency did not
survive checking whether it was a new dependency at all. The `#[cfg]` split is
gone, the guard is one function, and the hard-link tests that were `#[cfg(unix)]`
now compile everywhere.

CI is ubuntu-only, so the Windows arm is asserted by the shared implementation
rather than executed. That is a strictly better position than the case-insensitive
fallback, which CI did not execute either AND was known not to cover the case.

## Accepted limit — `flock` is node-local

Advisory locks are emulated per node on network filesystems rather than shared
across a cluster. Two machines sharing one `--spool-dir` over NFS can each
believe they own a sentinel, leaving the minimum-age veto as the only remaining
guard against one machine removing the other's live spool. This is a documented
limit of the flag, not a defect to fix in the extractor: give each machine its
own spool directory.
