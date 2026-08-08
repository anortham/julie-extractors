//! Parent-liveness watchdog for `scan --parent-pid`.
//!
//! The flag names the process that spawned this one. A background thread asks
//! the kernel who the parent is *now* and compares: when the spawner dies the
//! child is reparented to init, so a changed parent id is the orphan signal.
//! Asking about the current parent rather than probing a recorded id also
//! removes the process-id-reuse hazard outright — a recycled id can never
//! re-become our parent.
//!
//! Two deliberate limits:
//!
//! * The trip is a cooperative abort flag, never `std::process::exit`. Exiting
//!   skips `Drop` in every thread, and `Drop` is the only thing that removes the
//!   extraction spool — an exiting watchdog would leak exactly the file it was
//!   added to stop leaking. The scan checks the flag between extraction chunks
//!   and before it opens the artifact, and never once the write has started.
//! * Detecting a closed stdout instead is not viable here: a scan writes nothing
//!   to stdout until the report at the very end, so there is no mid-scan write to
//!   fail, and polling the descriptor for hangup needs `unsafe`, which the
//!   workspace forbids.
//!
//! `std` exposes no Windows equivalent of `parent_id`, so the watchdog is
//! Unix-only and the flag is an accepted no-op elsewhere; one caller argv then
//! works on every platform.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Cooperative abort signal shared with the parent-liveness thread.
#[derive(Clone, Default)]
pub(crate) struct ParentWatchdog {
    state: Arc<WatchdogState>,
}

#[derive(Default)]
struct WatchdogState {
    orphaned: AtomicBool,
    observed_parent_pid: AtomicU32,
}

impl ParentWatchdog {
    /// Probe once, then keep probing on a detached thread until the parent
    /// changes. Returns immediately; the thread is never joined and does not
    /// delay process exit.
    pub(crate) fn start(expected_parent_pid: u32) -> Self {
        let watchdog = Self::default();
        if watchdog.probe(expected_parent_pid) {
            return watchdog;
        }
        let polling = watchdog.clone();
        std::thread::spawn(move || {
            while !polling.probe(expected_parent_pid) {
                std::thread::sleep(POLL_INTERVAL);
            }
        });
        watchdog
    }

    /// `Acquire` pairs with the `Release` store in [`Self::probe`], so a thread
    /// that sees the trip is guaranteed to see the observed parent id that goes
    /// with it rather than the atomic's zero default.
    pub(crate) fn parent_exited(&self) -> bool {
        self.state.orphaned.load(Ordering::Acquire)
    }

    pub(crate) fn observed_parent_pid(&self) -> Option<u32> {
        self.parent_exited()
            .then(|| self.state.observed_parent_pid.load(Ordering::Relaxed))
    }

    fn probe(&self, expected_parent_pid: u32) -> bool {
        let Some(observed) = current_parent_pid() else {
            return true;
        };
        if !is_orphaned(expected_parent_pid, observed) {
            return false;
        }
        self.state
            .observed_parent_pid
            .store(observed, Ordering::Relaxed);
        self.state.orphaned.store(true, Ordering::Release);
        true
    }

    #[cfg(test)]
    pub(crate) fn tripped(observed_parent_pid: u32) -> Self {
        let watchdog = Self::default();
        watchdog
            .state
            .observed_parent_pid
            .store(observed_parent_pid, Ordering::Relaxed);
        watchdog.state.orphaned.store(true, Ordering::Release);
        watchdog
    }
}

pub(crate) fn is_orphaned(expected_parent_pid: u32, observed_parent_pid: u32) -> bool {
    expected_parent_pid != observed_parent_pid
}

#[cfg(unix)]
fn current_parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

#[cfg(not(unix))]
fn current_parent_pid() -> Option<u32> {
    None
}

#[allow(dead_code)]
pub(crate) fn process_status(pid: u32) -> julie_extract_artifact::store::PidStatus {
    if pid == std::process::id() {
        return julie_extract_artifact::store::PidStatus::Alive;
    }
    process_status_other(pid)
}

#[cfg(unix)]
#[allow(dead_code)]
fn process_status_other(pid: u32) -> julie_extract_artifact::store::PidStatus {
    let status = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if status.is_ok_and(|status| status.success()) {
        return julie_extract_artifact::store::PidStatus::Alive;
    }
    let status = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output();
    match status {
        Ok(output)
            if output.status.success() && output.stdout.iter().all(u8::is_ascii_whitespace) =>
        {
            julie_extract_artifact::store::PidStatus::Dead
        }
        Ok(output) if !output.status.success() => julie_extract_artifact::store::PidStatus::Dead,
        _ => julie_extract_artifact::store::PidStatus::Unknown,
    }
}

#[cfg(not(unix))]
fn process_status_other(_pid: u32) -> julie_extract_artifact::store::PidStatus {
    julie_extract_artifact::store::PidStatus::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_parent_id_is_not_an_orphan() {
        assert!(!is_orphaned(4242, 4242));
    }

    #[test]
    fn any_other_parent_id_is_an_orphan() {
        assert!(is_orphaned(4242, 1));
        assert!(is_orphaned(4242, 4243));
        assert!(is_orphaned(1, 0));
    }

    #[test]
    fn a_default_watchdog_never_reports_an_exit() {
        let watchdog = ParentWatchdog::default();

        assert!(!watchdog.parent_exited());
        assert_eq!(watchdog.observed_parent_pid(), None);
    }

    #[cfg(unix)]
    #[test]
    fn watching_the_real_parent_does_not_trip() {
        let watchdog = ParentWatchdog::start(std::os::unix::process::parent_id());

        assert!(!watchdog.parent_exited());
    }

    #[cfg(unix)]
    #[test]
    fn watching_a_process_that_is_not_the_parent_trips_on_the_first_probe() {
        let not_the_parent = std::os::unix::process::parent_id().wrapping_add(1);
        let watchdog = ParentWatchdog::start(not_the_parent);

        assert!(watchdog.parent_exited());
        assert_eq!(
            watchdog.observed_parent_pid(),
            Some(std::os::unix::process::parent_id())
        );
    }
}
