//! Live progress records for `scan --progress-file`.
//!
//! A scan spends its long phase growing a temporary spool and only opens the
//! artifact database near the end, so a consumer that watches artifact file
//! sizes sees nothing at all for that whole window and cannot tell a healthy
//! large scan from a wedged one. This file is that missing signal.
//!
//! The format is append-only JSONL, one unbuffered `write_all` per record, so
//! within one scan the file length never decreases and a consumer can treat
//! "length grew" as "the scan advanced" without parsing anything.
//!
//! `write_all` advances the file offset by whatever it managed to write before
//! failing, so a full disk can leave a half-written record in the middle of the
//! file. The sink remembers that and opens the next record with a newline, which
//! closes the truncated line: the damage stays confined to one droppable line and
//! every later record still parses.
//!
//! Absent flag means absent module: `scan` holds `Option<&ScanProgress>` and
//! never constructs one, so nothing is opened, written, or measured.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::paths::{PathPolicyError, invalid_path, reject_progress_file_collision};

pub(crate) const PROGRESS_SCHEMA_VERSION: u32 = 1;

const DEFAULT_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// Counters a scan advances while it works. Phase names mirror the report
/// profile's phase keys so the live signal and the final report agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Counter {
    Discovered,
    Supported,
    Extracted,
    Spooled,
}

struct ProgressSink {
    writer: Box<dyn Write + Send>,
    phase: &'static str,
    /// Set when a write failed part-way through a record, so the next record
    /// opens with the newline that closes the truncated line.
    torn: bool,
}

pub(crate) struct ScanProgress {
    sink: Mutex<ProgressSink>,
    started: Instant,
    interval_millis: u64,
    pid: u32,
    discovered: AtomicU64,
    supported: AtomicU64,
    extracted: AtomicU64,
    spooled: AtomicU64,
    next_due_millis: AtomicU64,
}

impl ScanProgress {
    /// Create the progress file for a scan writing `db_path`, re-running the
    /// collision guard once the file exists.
    ///
    /// The guard the caller already ran can only compare file identity for
    /// paths that are both there to be compared. When neither the progress file
    /// nor the artifact exists yet, creating the progress file is what makes
    /// them comparable — and on a case-insensitive volume it is also what makes
    /// them the same file, so `--db index.progress --progress-file
    /// INDEX.PROGRESS` would otherwise hand the scan a progress writer aimed at
    /// the artifact it is about to write.
    ///
    /// A file this call created is removed again on refusal, so a rejected argv
    /// does not leave an empty file sitting at the artifact's path for the next
    /// run to open.
    pub(crate) fn create_for_artifact(
        path: &Path,
        db_path: &Path,
    ) -> Result<Self, PathPolicyError> {
        let existed = path.exists();
        let progress = Self::create(path)?;
        let Err(error) = reject_progress_file_collision(path, db_path) else {
            return Ok(progress);
        };
        drop(progress);
        if !existed {
            let _ = std::fs::remove_file(path);
        }
        Err(error)
    }

    /// Private on purpose: creating a progress file without re-running the collision guard is the
    /// defect `create_for_artifact` exists to prevent, so argv-driven creation has one reachable door.
    fn create(path: &Path) -> Result<Self, PathPolicyError> {
        Self::create_with_interval(path, DEFAULT_PROGRESS_INTERVAL)
    }

    pub(crate) fn create_with_interval(
        path: &Path,
        interval: Duration,
    ) -> Result<Self, PathPolicyError> {
        let file = File::create(path).map_err(|error| {
            invalid_path(path, format!("progress file could not be created: {error}"))
        })?;
        Ok(Self::with_writer(Box::new(file), interval))
    }

    fn with_writer(writer: Box<dyn Write + Send>, interval: Duration) -> Self {
        Self {
            sink: Mutex::new(ProgressSink {
                writer,
                phase: "starting",
                torn: false,
            }),
            started: Instant::now(),
            interval_millis: duration_millis(interval),
            pid: std::process::id(),
            discovered: AtomicU64::new(0),
            supported: AtomicU64::new(0),
            extracted: AtomicU64::new(0),
            spooled: AtomicU64::new(0),
            next_due_millis: AtomicU64::new(0),
        }
    }

    /// Record entry into a scan phase. Unthrottled — a scan enters at most a
    /// handful of phases, and the phase change is itself the news.
    pub(crate) fn enter_phase(&self, phase: &'static str) {
        let Ok(mut sink) = self.sink.lock() else {
            return;
        };
        sink.phase = phase;
        self.write_record(&mut sink);
    }

    /// Advance a counter and, at most once per interval, write a record. Safe to
    /// call from every extraction worker: the counter is a relaxed add and only
    /// the single worker that wins the interval slot touches the file.
    pub(crate) fn advance(&self, counter: Counter, by: u64) {
        if by == 0 {
            return;
        }
        self.counter(counter).fetch_add(by, Ordering::Relaxed);
        if !self.claim_write_slot() {
            return;
        }
        let Ok(mut sink) = self.sink.lock() else {
            return;
        };
        self.write_record(&mut sink);
    }

    fn counter(&self, counter: Counter) -> &AtomicU64 {
        match counter {
            Counter::Discovered => &self.discovered,
            Counter::Supported => &self.supported,
            Counter::Extracted => &self.extracted,
            Counter::Spooled => &self.spooled,
        }
    }

    fn claim_write_slot(&self) -> bool {
        let now = duration_millis(self.started.elapsed());
        let due = self.next_due_millis.load(Ordering::Relaxed);
        if now < due {
            return false;
        }
        self.next_due_millis
            .compare_exchange(
                due,
                now.saturating_add(self.interval_millis),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    fn write_record(&self, sink: &mut ProgressSink) {
        let record = json!({
            "progress_schema_version": PROGRESS_SCHEMA_VERSION,
            "pid": self.pid,
            "phase": sink.phase,
            "elapsed_ms": duration_millis(self.started.elapsed()),
            "files_discovered": self.discovered.load(Ordering::Relaxed),
            "files_supported": self.supported.load(Ordering::Relaxed),
            "files_extracted": self.extracted.load(Ordering::Relaxed),
            "files_spooled": self.spooled.load(Ordering::Relaxed),
        });
        let Ok(record) = serde_json::to_vec(&record) else {
            return;
        };
        let mut line = Vec::with_capacity(record.len() + 2);
        if sink.torn {
            line.push(b'\n');
        }
        line.extend_from_slice(&record);
        line.push(b'\n');
        sink.torn = sink.writer.write_all(&line).is_err();
        let _ = sink.writer.flush();
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    #[derive(Clone, Default)]
    struct TearingWriter {
        written: Arc<Mutex<Vec<u8>>>,
        tear_next: Arc<AtomicBool>,
    }

    impl Write for TearingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut written = self.written.lock().unwrap();
            if self.tear_next.swap(false, Ordering::Relaxed) {
                written.extend_from_slice(&buf[..buf.len() / 2]);
                return Err(io::Error::new(io::ErrorKind::StorageFull, "no space"));
            }
            written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn records(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn every_record_carries_the_schema_version_the_pid_and_the_phase() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("scan.progress");
        let progress = ScanProgress::create_with_interval(&path, Duration::ZERO).unwrap();

        progress.enter_phase("discovery");
        progress.advance(Counter::Discovered, 3);

        let records = records(&path);
        assert_eq!(records.len(), 2);
        for record in &records {
            assert_eq!(record["progress_schema_version"], PROGRESS_SCHEMA_VERSION);
            assert_eq!(record["pid"], std::process::id());
            assert_eq!(record["phase"], "discovery");
            assert!(record["elapsed_ms"].as_u64().is_some());
        }
        assert_eq!(records[1]["files_discovered"], 3);
    }

    #[test]
    fn counters_never_decrease_across_records() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("scan.progress");
        let progress = ScanProgress::create_with_interval(&path, Duration::ZERO).unwrap();

        for _ in 0..5 {
            progress.advance(Counter::Spooled, 2);
        }

        let spooled = records(&path)
            .iter()
            .map(|record| record["files_spooled"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(spooled, vec![2, 4, 6, 8, 10]);
    }

    #[test]
    fn nothing_is_written_when_no_counter_advances() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("scan.progress");
        let progress = ScanProgress::create_with_interval(&path, Duration::ZERO).unwrap();

        progress.advance(Counter::Extracted, 0);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn a_nonzero_interval_admits_one_record_per_interval() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("scan.progress");
        let progress =
            ScanProgress::create_with_interval(&path, Duration::from_secs(3600)).unwrap();

        for _ in 0..1000 {
            progress.advance(Counter::Extracted, 1);
        }

        let records = records(&path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["files_extracted"], 1);
    }

    #[test]
    fn concurrent_workers_do_not_interleave_partial_records() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("scan.progress");
        let progress = ScanProgress::create_with_interval(&path, Duration::ZERO).unwrap();

        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..64 {
                        progress.advance(Counter::Extracted, 1);
                    }
                });
            }
        });

        let extracted = records(&path)
            .iter()
            .map(|record| record["files_extracted"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert!(!extracted.is_empty());
        assert!(
            extracted.windows(2).all(|pair| pair[0] <= pair[1]),
            "records must stay non-decreasing: {extracted:?}"
        );
        assert!(extracted.iter().all(|value| *value <= 8 * 64));
        assert_eq!(progress.extracted.load(Ordering::Relaxed), 8 * 64);
    }

    #[test]
    fn a_half_written_record_is_closed_by_the_next_one_so_later_lines_still_parse() {
        let writer = TearingWriter::default();
        let written = writer.written.clone();
        let tear_next = writer.tear_next.clone();
        let progress = ScanProgress::with_writer(Box::new(writer), Duration::ZERO);

        progress.advance(Counter::Spooled, 1);
        tear_next.store(true, Ordering::Relaxed);
        progress.advance(Counter::Spooled, 1);
        progress.advance(Counter::Spooled, 1);
        progress.advance(Counter::Spooled, 1);

        let text = String::from_utf8(written.lock().unwrap().clone()).unwrap();
        let lines = text.lines().collect::<Vec<_>>();
        let spooled = lines
            .iter()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .map(|record| record["files_spooled"].as_u64().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            spooled,
            vec![1, 3, 4],
            "only the half-written record may be lost: {text:?}"
        );
        assert_eq!(
            lines.len() - spooled.len(),
            1,
            "the damage must stay confined to one line: {text:?}"
        );
    }

    #[test]
    fn creating_a_progress_file_under_a_missing_directory_is_a_typed_path_error() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("absent").join("scan.progress");

        assert!(matches!(
            ScanProgress::create(&path),
            Err(PathPolicyError::InvalidPath { .. })
        ));
    }
}
