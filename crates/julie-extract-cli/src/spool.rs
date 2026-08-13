//! Placement, ownership, and startup cleanup of the extraction spool file.
//!
//! A scan that is hard-killed (OOM, SIGKILL) never runs `Drop`, so its spool
//! file survives. Ownership is proved with an advisory lock rather than a
//! recorded process id: the kernel drops the lock when the owning process dies
//! however it dies, and there is no process-id-reuse window to get wrong.
//! `unsafe_code = "forbid"` rules out the alternatives (`kill(pid, 0)`,
//! `OpenProcess`), and they would be less correct anyway.
//!
//! The lock lives on a sibling `<spool>.lock` sentinel and never on the spool's
//! own byte range. Windows file locks are mandatory over the locked range, so a
//! lock taken on the spool through a second handle makes the spool's own writer
//! fail with a lock violation; the sentinel keeps one mechanism correct on every
//! platform. The sentinel is created and locked before the spool file exists, so
//! a spool never exists whose sentinel is not already locked.
//!
//! Removal candidacy is therefore structural rather than a matter of operator
//! discipline: only a locked-sentinel-backed spool name is a candidate, so a
//! spool written without `--spool-dir`, or on a filesystem where locking is
//! unavailable, can never be removed by anyone. Retirement runs the same way in
//! reverse: the sentinel is removed only once its spool is gone, because it is
//! the only thing that keeps a surviving spool removable.
//!
//! Accepted limit: `flock` is node-local, and network filesystems emulate it per
//! node rather than across the cluster. Two machines sharing one `--spool-dir`
//! over NFS can each believe they own a sentinel, and only the minimum-age veto
//! stands between them. Give each machine its own spool directory.
//!
//! Both halves are opt-in through `--spool-dir`. Without the flag the spool
//! lands in the system temporary directory with no sentinel and nothing is ever
//! removed, which is byte-for-byte what a scan did before the flag existed.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use julie_extract_artifact::writer::{ArtifactFileSpool, ArtifactSpoolError};

/// Spools no sentinel can vouch for. Used without `--spool-dir`, and as the
/// fallback when a sentinel cannot be locked. Never a removal candidate.
const SPOOL_PREFIX: &str = "julie-extract-scan-spool-";

/// Spools that own a locked sentinel. The only shape removal will consider.
const OWNED_SPOOL_PREFIX: &str = "julie-extract-scan-owned-spool-";

const SPOOL_SUFFIX: &str = ".jsonl";
const SENTINEL_SUFFIX: &str = ".lock";

/// A sentinel is only a removal candidate once it is older than this. The lock
/// is the removal authority; this window exists because a sentinel is created
/// and then locked as two separate operations, and a candidate younger than the
/// window may belong to an owner that has not reached its lock yet.
const SPOOL_REAP_MIN_AGE: Duration = Duration::from_secs(5);

/// Whether a live process still owns a candidate spool's sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpoolOwner {
    /// The advisory lock was free, so no live process holds this spool.
    Unowned,
    /// The advisory lock is held, so the owning scan is still running.
    Held,
    /// The probe itself failed, so ownership is unknown.
    Unknown,
}

/// The scan's own spool file plus the sentinel that marks it as owned.
///
/// Cleanup lives here rather than on the caller's aggregate so that every early
/// return between creating the spool and handing it upward still removes it.
pub(crate) struct ScanSpool {
    spool: ArtifactFileSpool,
    sentinel: Option<SpoolSentinel>,
    ownership_lock_unavailable: bool,
}

/// The `<spool>.lock` file that carries the advisory lock for one spool.
///
/// Dropping it releases the lock. Removing the file is a separate decision the
/// owner makes, because a sentinel is the only thing that keeps a leftover spool
/// removable: taking it away while its spool survives converts a reapable leak
/// into a permanent one.
struct SpoolSentinel {
    path: PathBuf,
    file: File,
}

impl ScanSpool {
    pub(crate) fn file_spool_mut(&mut self) -> &mut ArtifactFileSpool {
        &mut self.spool
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        self.spool.path()
    }

    #[cfg(test)]
    pub(crate) fn sentinel_path(&self) -> Option<&Path> {
        self.sentinel
            .as_ref()
            .map(|sentinel| sentinel.path.as_path())
    }

    pub(crate) fn len(&self) -> usize {
        self.spool.len()
    }

    /// Whether `--spool-dir` was given but its sentinel could not be locked, so
    /// this spool fell back to a name no reaper will ever consider. The caller
    /// surfaces it as a warning: an operator who adopted the flag to stop a leak
    /// otherwise has no way to learn the protection is inert.
    pub(crate) fn ownership_lock_unavailable(&self) -> bool {
        self.ownership_lock_unavailable
    }

    #[cfg(test)]
    pub(crate) fn file_spool(&self) -> &ArtifactFileSpool {
        &self.spool
    }

    fn retire(&mut self, remove_spool: impl Fn(&Path) -> bool) {
        let _ = self.spool.finish();
        let spool_gone = remove_spool(self.spool.path());
        let Some(sentinel) = self.sentinel.take() else {
            return;
        };
        // Released and removed last: while the sentinel is still locked, no
        // reaper can act on a pair this scan is in the middle of retiring.
        sentinel.release(spool_gone);
    }
}

impl Drop for ScanSpool {
    fn drop(&mut self) {
        self.retire(remove_spool_file);
    }
}

impl SpoolSentinel {
    fn release(self, spool_gone: bool) {
        let _ = self.file.unlock();
        drop(self.file);
        if spool_gone {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Whether the spool is gone once this returns. A path that was already absent
/// counts: the sentinel's job is to keep a SURVIVING spool removable.
fn remove_spool_file(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

pub(crate) fn create_scan_spool(spool_dir: Option<&Path>) -> Result<ScanSpool, ArtifactSpoolError> {
    create_scan_spool_with(spool_dir, ArtifactFileSpool::create)
}

fn create_scan_spool_with(
    spool_dir: Option<&Path>,
    create_spool: impl Fn(PathBuf) -> Result<ArtifactFileSpool, ArtifactSpoolError>,
) -> Result<ScanSpool, ArtifactSpoolError> {
    let pid = std::process::id();
    let nanos = spool_nanos();
    let Some(dir) = spool_dir else {
        return unowned_scan_spool(&std::env::temp_dir(), pid, nanos, false, create_spool);
    };
    // A directory that cannot carry a lock (some NFS and FUSE scratch mounts
    // return ENOLCK) falls back to the non-candidate name rather than failing the
    // scan: the flag exists to make concurrent scans safer, and refusing to run
    // would trade a leak for an outage. The fallback spool is still removed by
    // this scan's own `Drop`, and no reaper can ever consider it.
    let Some(sentinel) = acquire_sentinel(dir, pid, nanos) else {
        return unowned_scan_spool(dir, pid, nanos, true, create_spool);
    };
    // The sentinel exists and is locked before this call, so no spool of the
    // owned shape ever exists without a locked sentinel vouching for it.
    match create_spool(dir.join(owned_spool_file_name(pid, nanos))) {
        Ok(spool) => Ok(ScanSpool {
            spool,
            sentinel: Some(sentinel),
            ownership_lock_unavailable: false,
        }),
        Err(error) => {
            sentinel.release(true);
            Err(error)
        }
    }
}

fn unowned_scan_spool(
    dir: &Path,
    pid: u32,
    nanos: u128,
    ownership_lock_unavailable: bool,
    create_spool: impl Fn(PathBuf) -> Result<ArtifactFileSpool, ArtifactSpoolError>,
) -> Result<ScanSpool, ArtifactSpoolError> {
    let spool = create_spool(dir.join(unowned_spool_file_name(pid, nanos)))?;
    Ok(ScanSpool {
        spool,
        sentinel: None,
        ownership_lock_unavailable,
    })
}

fn acquire_sentinel(dir: &Path, pid: u32, nanos: u128) -> Option<SpoolSentinel> {
    let path = dir.join(sentinel_file_name(pid, nanos));
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .ok()?;
    if file.try_lock().is_err() {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(SpoolSentinel { path, file })
}

pub(crate) fn unowned_spool_file_name(pid: u32, nanos: u128) -> String {
    format!("{SPOOL_PREFIX}{pid}-{nanos}{SPOOL_SUFFIX}")
}

pub(crate) fn owned_spool_file_name(pid: u32, nanos: u128) -> String {
    format!("{OWNED_SPOOL_PREFIX}{pid}-{nanos}{SPOOL_SUFFIX}")
}

pub(crate) fn sentinel_file_name(pid: u32, nanos: u128) -> String {
    format!("{}{SENTINEL_SUFFIX}", owned_spool_file_name(pid, nanos))
}

/// Whether a file name is one this module creates — either spool shape, or a
/// sentinel. Discovery uses it to keep a spool directory placed inside `--root`
/// out of the scan; `jsonl` is a supported extension, so a surviving spool would
/// otherwise be extracted as if it were source.
pub(crate) fn is_spool_artifact_name(file_name: &str) -> bool {
    let spool = file_name.strip_suffix(SENTINEL_SUFFIX).unwrap_or(file_name);
    spool_name_pid(spool, SPOOL_PREFIX).is_some()
        || spool_name_pid(spool, OWNED_SPOOL_PREFIX).is_some()
}

/// The spool file name a sentinel vouches for, or `None` when the name is not a
/// sentinel for an owned spool. Iterating sentinels rather than spools is what
/// makes candidacy structurally imply lock ownership: a spool with no sentinel
/// is never reached, so it can never be removed.
fn owned_spool_name_for_sentinel(file_name: &str) -> Option<&str> {
    let spool = file_name.strip_suffix(SENTINEL_SUFFIX)?;
    spool_name_pid(spool, OWNED_SPOOL_PREFIX).map(|_| spool)
}

/// The process id embedded in a spool file name, or `None` when the name is not
/// the requested shape. The id itself is diagnostic: it proves the name is a
/// spool, and never authorizes removal — the sentinel's lock does that.
fn spool_name_pid(file_name: &str, prefix: &str) -> Option<u32> {
    let body = file_name.strip_prefix(prefix)?.strip_suffix(SPOOL_SUFFIX)?;
    let (pid, nanos) = body.split_once('-')?;
    if nanos.is_empty() || !nanos.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

pub(crate) fn should_reap(owner: SpoolOwner, age: Option<Duration>) -> bool {
    owner == SpoolOwner::Unowned && age.is_some_and(|age| age >= SPOOL_REAP_MIN_AGE)
}

/// Remove spool files in `dir` that no live scan owns, together with their
/// sentinels. Best effort throughout: a foreign-owned, read-only, or missing
/// directory leaves the scan unaffected.
pub(crate) fn reap_unowned_spools(dir: &Path) {
    reap_unowned_spools_with(dir, remove_spool_file);
}

/// [`reap_unowned_spools`] with the spool removal injected — the seam that lets a
/// test observe the sentinel's lock state at the moment the spool is unlinked,
/// which is the property the claim exists to provide and the only point at which
/// it is observable.
fn reap_unowned_spools_with(dir: &Path, mut remove: impl FnMut(&Path) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let sentinel = entry.path();
        let Some(spool) = sentinel
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(owned_spool_name_for_sentinel)
            .map(|name| dir.join(name))
        else {
            continue;
        };
        let claim = claim_sentinel(&sentinel);
        if !should_reap(claim.owner(), file_age(&sentinel, now)) {
            continue;
        }
        // The sentinel is the only thing that makes its spool a candidate, so
        // removing it after a failed spool removal would turn one transient
        // failure into a leak nothing can ever clean up.
        if remove(&spool) {
            let _ = std::fs::remove_file(&sentinel);
        }
        drop(claim);
    }
}

/// A reap candidate's ownership, holding the lock for as long as the value lives
/// when the answer is [`SpoolOwner::Unowned`].
///
/// Deciding under the lock and then releasing it before deleting would leave a
/// window in which the pair is unlocked and still present. Nothing legitimate
/// occupies that window today — sentinel names embed the owner's process id and
/// a nanosecond stamp, so a starting scan always creates its own rather than
/// adopting this one — but that is an argument about a NAMING scheme guarding a
/// deletion, and the reason this flag reaps by lock at all is that a naming
/// scheme is not evidence of liveness. Holding the claim through the unlink
/// makes the deletion true by the same thing that authorized it.
enum SentinelClaim {
    /// The lock was free and is held here until this value is dropped.
    Locked(File),
    Held,
    Unknown,
}

impl SentinelClaim {
    fn owner(&self) -> SpoolOwner {
        match self {
            Self::Locked(_) => SpoolOwner::Unowned,
            Self::Held => SpoolOwner::Held,
            Self::Unknown => SpoolOwner::Unknown,
        }
    }
}

impl Drop for SentinelClaim {
    fn drop(&mut self) {
        if let Self::Locked(file) = self {
            let _ = file.unlock();
        }
    }
}

/// Read+write, matching [`acquire_sentinel`]. `std` documents that on a handle
/// not open for writing it is unspecified whether taking a lock returns an
/// error, and an error here maps to `Unknown`, which vetoes every reap — so a
/// read-only probe would make the whole flag a silent no-op on such a platform.
fn claim_sentinel(path: &Path) -> SentinelClaim {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return SentinelClaim::Unknown;
    };
    match file.try_lock() {
        Ok(()) => SentinelClaim::Locked(file),
        Err(std::fs::TryLockError::WouldBlock) => SentinelClaim::Held,
        Err(std::fs::TryLockError::Error(_)) => SentinelClaim::Unknown,
    }
}

/// [`claim_sentinel`] reduced to its verdict, releasing any lock it took. The
/// reaper must not use this: the whole point of the claim is that the lock
/// outlives the decision.
#[cfg(test)]
fn probe_sentinel_owner(path: &Path) -> SpoolOwner {
    claim_sentinel(path).owner()
}

fn file_age(path: &Path, now: SystemTime) -> Option<Duration> {
    now.duration_since(path.metadata().ok()?.modified().ok()?)
        .ok()
}

fn spool_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::TempDir;

    fn plant_owned_pair(dir: &Path, pid: u32, age: Duration) -> (PathBuf, PathBuf) {
        let spool = dir.join(owned_spool_file_name(pid, 1_754_000_000_000_000_000));
        let sentinel = dir.join(sentinel_file_name(pid, 1_754_000_000_000_000_000));
        File::create(&spool).unwrap();
        File::create(&sentinel)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
        (spool, sentinel)
    }

    fn plant_orphan_spool(dir: &Path, name: String, age: Duration) -> PathBuf {
        let path = dir.join(name);
        File::create(&path)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
        path
    }

    #[test]
    fn a_spool_created_without_a_spool_dir_lands_in_the_system_temporary_directory() {
        let spool = create_scan_spool(None).unwrap();

        assert_eq!(spool.path().parent(), Some(std::env::temp_dir().as_path()));
        assert_eq!(spool.sentinel_path(), None);
        assert_eq!(
            owned_spool_name_for_sentinel(&format!(
                "{}{SENTINEL_SUFFIX}",
                spool.path().file_name().unwrap().to_str().unwrap()
            )),
            None,
            "a flagless spool name must never be a removal candidate"
        );
    }

    #[test]
    fn a_spool_created_with_a_spool_dir_locks_a_sibling_sentinel_inside_that_directory() {
        let temp = TempDir::new().unwrap();
        let spool = create_scan_spool(Some(temp.path())).unwrap();

        let sentinel = spool.sentinel_path().unwrap().to_path_buf();
        assert_eq!(spool.path().parent(), Some(temp.path()));
        assert_eq!(sentinel.parent(), Some(temp.path()));
        assert_eq!(
            sentinel.file_name().unwrap().to_str().unwrap(),
            format!(
                "{}{SENTINEL_SUFFIX}",
                spool.path().file_name().unwrap().to_str().unwrap()
            )
        );
        assert_eq!(probe_sentinel_owner(&sentinel), SpoolOwner::Held);
    }

    #[test]
    fn a_spool_dir_that_cannot_carry_a_lock_still_gets_a_spool_no_one_can_reap() {
        let temp = TempDir::new().unwrap();
        let spool = unowned_scan_spool(temp.path(), 7, 9, true, ArtifactFileSpool::create).unwrap();
        let path = spool.path().to_path_buf();

        assert_eq!(path.parent(), Some(temp.path()));
        assert_eq!(spool.sentinel_path(), None);
        assert!(
            spool.ownership_lock_unavailable(),
            "the caller must be able to warn that leak protection is inert"
        );
        reap_unowned_spools(temp.path());
        assert!(
            path.exists(),
            "the fallback spool must never be a candidate"
        );

        drop(spool);
        assert!(!path.exists(), "its own Drop still removes it");
    }

    #[test]
    fn a_flagless_spool_does_not_report_the_lock_as_unavailable() {
        let spool = create_scan_spool(None).unwrap();

        assert!(!spool.ownership_lock_unavailable());
    }

    #[test]
    fn a_locked_spool_does_not_report_the_lock_as_unavailable() {
        let temp = TempDir::new().unwrap();
        let spool = create_scan_spool(Some(temp.path())).unwrap();

        assert!(!spool.ownership_lock_unavailable());
    }

    #[test]
    fn the_sentinel_is_locked_before_the_spool_file_is_created() {
        let temp = TempDir::new().unwrap();
        let observed = Cell::new(None);
        let spool = create_scan_spool_with(Some(temp.path()), |path| {
            observed.set(Some((
                path.exists(),
                sentinel_entries(temp.path())
                    .into_iter()
                    .map(|sentinel| probe_sentinel_owner(&sentinel))
                    .collect::<Vec<_>>(),
            )));
            ArtifactFileSpool::create(path)
        })
        .unwrap();

        let (spool_existed, sentinels) = observed.into_inner().unwrap();
        assert!(!spool_existed, "the spool must not exist yet");
        assert_eq!(
            sentinels,
            vec![SpoolOwner::Held],
            "its sentinel must already exist and be locked"
        );
        drop(spool);
    }

    #[test]
    fn the_sentinel_is_still_locked_while_the_spool_is_removed_and_goes_away_after() {
        let temp = TempDir::new().unwrap();
        let mut spool = create_scan_spool(Some(temp.path())).unwrap();
        let sentinel = spool.sentinel_path().unwrap().to_path_buf();
        let observed = Cell::new(None);

        spool.retire(|path| {
            observed.set(Some((sentinel.exists(), probe_sentinel_owner(&sentinel))));
            remove_spool_file(path)
        });

        assert_eq!(
            observed.into_inner(),
            Some((true, SpoolOwner::Held)),
            "the sentinel must outlive the spool removal, still locked"
        );
        assert!(!sentinel.exists(), "and be removed once the spool is gone");
    }

    #[test]
    fn reaping_still_holds_the_sentinel_lock_while_it_unlinks_the_spool() {
        let temp = TempDir::new().unwrap();
        let (_spool, sentinel) = plant_owned_pair(temp.path(), 41, SPOOL_REAP_MIN_AGE * 100);
        let observed = Cell::new(None);

        reap_unowned_spools_with(temp.path(), |path| {
            observed.set(Some(probe_sentinel_owner(&sentinel)));
            remove_spool_file(path)
        });

        assert_eq!(
            observed.into_inner(),
            Some(SpoolOwner::Held),
            "releasing the lock before the unlink leaves the pair unlocked and present"
        );
        assert!(!sentinel.exists());
    }

    #[test]
    fn a_sentinel_whose_spool_survives_is_kept_so_the_pair_stays_reapable() {
        let temp = TempDir::new().unwrap();
        let mut spool = create_scan_spool(Some(temp.path())).unwrap();
        let sentinel = spool.sentinel_path().unwrap().to_path_buf();

        spool.retire(|_| false);

        assert!(
            sentinel.exists(),
            "removing the sentinel would make the surviving spool unreapable forever"
        );
        assert_eq!(
            probe_sentinel_owner(&sentinel),
            SpoolOwner::Unowned,
            "the lock is still released so a later scan can reap the pair"
        );
    }

    #[test]
    fn reaping_keeps_a_sentinel_whose_spool_could_not_be_removed() {
        let temp = TempDir::new().unwrap();
        let (spool, sentinel) = plant_owned_pair(temp.path(), 11, SPOOL_REAP_MIN_AGE * 100);
        std::fs::remove_file(&spool).unwrap();
        std::fs::create_dir(&spool).unwrap();
        std::fs::write(spool.join("blocker"), b"not removable as a file").unwrap();

        reap_unowned_spools(temp.path());

        assert!(spool.exists(), "the spool removal must have failed");
        assert!(
            sentinel.exists(),
            "keeping the sentinel is what keeps the leftover reapable"
        );
    }

    fn sentinel_entries(dir: &Path) -> Vec<PathBuf> {
        let mut paths = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(SENTINEL_SUFFIX))
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    #[test]
    fn the_advisory_lock_never_touches_the_spool_data_file() {
        let temp = TempDir::new().unwrap();
        let mut spool = create_scan_spool(Some(temp.path())).unwrap();

        // A whole-file lock is mandatory on Windows, so a lock covering the
        // spool's own bytes fails the spool's own writer and every --spool-dir
        // scan with it. The spool must stay lockable by any other handle.
        assert_eq!(probe_sentinel_owner(spool.path()), SpoolOwner::Unowned);
        assert_eq!(
            probe_sentinel_owner(spool.sentinel_path().unwrap()),
            SpoolOwner::Held
        );
        spool.file_spool_mut().finish().unwrap();
    }

    #[test]
    fn a_spool_without_a_sentinel_is_never_reaped() {
        let temp = TempDir::new().unwrap();
        let owned_shape = plant_orphan_spool(
            temp.path(),
            owned_spool_file_name(11, 1_754_000_000_000_000_000),
            SPOOL_REAP_MIN_AGE * 100,
        );
        let flagless_shape = plant_orphan_spool(
            temp.path(),
            unowned_spool_file_name(22, 1_754_000_000_000_000_000),
            SPOOL_REAP_MIN_AGE * 100,
        );

        reap_unowned_spools(temp.path());

        assert!(
            owned_shape.exists(),
            "a spool whose sentinel is absent cannot be proved unowned"
        );
        assert!(
            flagless_shape.exists(),
            "a spool written without --spool-dir is never a candidate"
        );
    }

    #[test]
    fn reaping_removes_an_unlocked_pair_and_keeps_locked_young_and_foreign_files() {
        let temp = TempDir::new().unwrap();
        let (unowned_spool, unowned_sentinel) =
            plant_owned_pair(temp.path(), 11, SPOOL_REAP_MIN_AGE * 100);
        let (held_spool, held_sentinel) =
            plant_owned_pair(temp.path(), 22, SPOOL_REAP_MIN_AGE * 100);
        let (young_spool, young_sentinel) = plant_owned_pair(temp.path(), 33, Duration::ZERO);
        let foreign = temp.path().join("artifact.sqlite");
        std::fs::write(&foreign, b"not a spool").unwrap();

        let holder = File::open(&held_sentinel).unwrap();
        holder.lock().unwrap();
        reap_unowned_spools(temp.path());
        holder.unlock().unwrap();

        assert!(
            !unowned_spool.exists(),
            "an unowned spool should be removed"
        );
        assert!(!unowned_sentinel.exists(), "its sentinel goes with it");
        assert!(held_spool.exists(), "a locked spool must survive");
        assert!(held_sentinel.exists());
        assert!(
            young_spool.exists(),
            "a sentinel younger than the creation window must survive"
        );
        assert!(young_sentinel.exists());
        assert!(foreign.exists(), "unrelated files must be left alone");
    }

    #[test]
    fn reaping_never_removes_a_live_scans_spool() {
        let temp = TempDir::new().unwrap();
        let spool = create_scan_spool(Some(temp.path())).unwrap();
        let path = spool.path().to_path_buf();
        let sentinel = spool.sentinel_path().unwrap().to_path_buf();
        // Windows needs write access to set a timestamp; a read-only handle fails with
        // PermissionDenied. Unix allows it on a read-only handle, which is why this only ever
        // failed on Windows.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&sentinel)
            .unwrap()
            .set_modified(SystemTime::now() - SPOOL_REAP_MIN_AGE * 100)
            .unwrap();

        reap_unowned_spools(temp.path());

        assert!(path.exists(), "the lock outranks the age veto");
        assert!(sentinel.exists());
    }

    #[test]
    fn reaping_a_missing_directory_is_silent() {
        let temp = TempDir::new().unwrap();
        reap_unowned_spools(&temp.path().join("absent"));
    }

    #[test]
    fn dropping_a_scan_spool_removes_the_spool_and_its_sentinel() {
        let temp = TempDir::new().unwrap();
        let spool = create_scan_spool(Some(temp.path())).unwrap();
        let path = spool.path().to_path_buf();
        let sentinel = spool.sentinel_path().unwrap().to_path_buf();
        assert!(path.exists());
        assert!(sentinel.exists());

        drop(spool);

        assert!(!path.exists());
        assert!(!sentinel.exists());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn only_an_unowned_sentinel_past_the_creation_window_is_reaped() {
        let old = Some(SPOOL_REAP_MIN_AGE * 100);
        let young = Some(Duration::from_millis(1));

        assert!(should_reap(SpoolOwner::Unowned, old));
        assert!(!should_reap(SpoolOwner::Unowned, young));
        assert!(!should_reap(SpoolOwner::Unowned, None));
        assert!(!should_reap(SpoolOwner::Held, old));
        assert!(!should_reap(SpoolOwner::Unknown, old));
    }

    #[test]
    fn a_sentinel_name_reads_past_the_hyphens_in_the_name_prefix() {
        assert_eq!(
            owned_spool_name_for_sentinel(
                "julie-extract-scan-owned-spool-12345-1754000000000000000.jsonl.lock"
            ),
            Some("julie-extract-scan-owned-spool-12345-1754000000000000000.jsonl")
        );
    }

    #[test]
    fn names_that_are_not_owned_sentinels_are_not_candidates() {
        for name in [
            "julie-extract-scan-owned-spool-12345-1754000000000000000.jsonl",
            "julie-extract-scan-spool-12345-1754000000000000000.jsonl.lock",
            "julie-extract-scan-owned-spool-12345.jsonl.lock",
            "julie-extract-scan-owned-spool-12345-.jsonl.lock",
            "julie-extract-scan-owned-spool--1754.jsonl.lock",
            "julie-extract-scan-owned-spool-abc-1754.jsonl.lock",
            "julie-extract-scan-owned-spool-12345-17a4.jsonl.lock",
            "julie-extract-scan-owned-spool-12345-1754-9.jsonl.lock",
            "artifact.sqlite",
        ] {
            assert_eq!(
                owned_spool_name_for_sentinel(name),
                None,
                "{name} must not be a candidate"
            );
        }
    }

    #[test]
    fn every_name_this_module_creates_is_recognized_as_a_spool_artifact() {
        for name in [
            unowned_spool_file_name(1, 2),
            owned_spool_file_name(1, 2),
            sentinel_file_name(1, 2),
        ] {
            assert!(is_spool_artifact_name(&name), "{name} must be recognized");
        }
        for name in ["artifact.sqlite", "scan.progress", "lib.rs", "data.jsonl"] {
            assert!(!is_spool_artifact_name(name), "{name} must not match");
        }
    }
}
