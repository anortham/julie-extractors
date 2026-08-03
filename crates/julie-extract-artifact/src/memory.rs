//! Total-physical-memory probe used to size the bulk connection's page cache.
//!
//! The scan's resolution phase does random b-tree reads and re-dirtying over a
//! working set that grows with the artifact (multi-GiB on large repos). A fixed
//! small cache thrashes: the 2026-08-03 dotnet/runtime baseline measured the
//! 128 MiB cache spending ~90% of resolution blocked in `pread`/`pwrite`, and
//! raising it to 8 GiB alone cut the cold scan from 76.3 to 47.0 minutes.
//!
//! The workspace forbids `unsafe`, so the probe uses safe primitives only:
//! procfs on Linux and a subprocess on macOS/Windows, cached per process so
//! the cost is paid once, not per opened writer.

use std::sync::OnceLock;

/// Total physical memory in bytes, or `None` when the platform offers no
/// probe or the probe fails. Probed once per process.
pub(crate) fn total_memory_bytes() -> Option<u64> {
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(imp::total_memory_bytes)
}

/// Cache-size pragma value (negative KiB) for a machine with `total_memory`
/// bytes: an eighth of physical memory, clamped to [512 MiB, 8 GiB]. The
/// ceiling is the empirically validated size; the floor keeps small machines
/// safe while still quadrupling the historical 128 MiB fixed cache. A failed
/// probe gets the floor.
pub(crate) fn bulk_cache_kib_for(total_memory: Option<u64>) -> i64 {
    const MIN_KIB: i64 = 512 * 1024;
    const MAX_KIB: i64 = 8 * 1024 * 1024;
    let total_kib = total_memory.map_or(0, |bytes| (bytes / 1024) as i64);
    -(total_kib / 8).clamp(MIN_KIB, MAX_KIB)
}

/// Resolve the pragma value, honoring an operator's `JULIE_BULK_CACHE_KIB`
/// override verbatim (SQLite semantics: negative = KiB, positive = pages).
pub(crate) fn bulk_cache_size_kib() -> i64 {
    match std::env::var("JULIE_BULK_CACHE_KIB")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
    {
        Some(value) => value,
        None => bulk_cache_kib_for(total_memory_bytes()),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub(super) fn total_memory_bytes() -> Option<u64> {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib * 1024)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub(super) fn total_memory_bytes() -> Option<u64> {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let bytes: u64 = String::from_utf8(output.stdout).ok()?.trim().parse().ok()?;
        (bytes > 0).then_some(bytes)
    }
}

#[cfg(windows)]
mod imp {
    pub(super) fn total_memory_bytes() -> Option<u64> {
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let bytes: u64 = String::from_utf8(output.stdout).ok()?.trim().parse().ok()?;
        (bytes > 0).then_some(bytes)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub(super) fn total_memory_bytes() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_physical_memory_on_supported_platforms() {
        let total = total_memory_bytes().expect("probe should work on CI platforms");
        assert!(total >= 1024 * 1024 * 1024, "implausibly small: {total}");
    }

    #[test]
    fn cache_is_an_eighth_of_memory_within_bounds() {
        assert_eq!(
            bulk_cache_kib_for(Some(64 * 1024 * 1024 * 1024)),
            -(8 * 1024 * 1024)
        );
        assert_eq!(
            bulk_cache_kib_for(Some(16 * 1024 * 1024 * 1024)),
            -(2 * 1024 * 1024)
        );
    }

    #[test]
    fn cache_clamps_to_floor_and_ceiling() {
        assert_eq!(bulk_cache_kib_for(Some(1024 * 1024 * 1024)), -(512 * 1024));
        assert_eq!(
            bulk_cache_kib_for(Some(1024 * 1024 * 1024 * 1024)),
            -(8 * 1024 * 1024)
        );
    }

    #[test]
    fn failed_probe_gets_the_floor() {
        assert_eq!(bulk_cache_kib_for(None), -(512 * 1024));
    }
}
