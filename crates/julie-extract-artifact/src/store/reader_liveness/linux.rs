use std::path::Path;

use super::{ProcessIdentityObservation, ProcessIdentityUnknownReason, ProcessInstanceIdentity};

pub(super) fn inspect(pid: u32) -> ProcessIdentityObservation {
    inspect_at(Path::new("/proc"), pid)
}

fn inspect_at(proc_root: &Path, pid: u32) -> ProcessIdentityObservation {
    inspect_at_with_probe(proc_root, pid, std::process::id(), &kernel_pid_observation)
}

pub(super) fn validate_identity_domain(
    original_birth_identity: &str,
) -> Result<(), ProcessIdentityUnknownReason> {
    validate_identity_domain_at(
        Path::new("/proc"),
        original_birth_identity,
        std::process::id(),
    )
}

fn inspect_at_with_probe(
    proc_root: &Path,
    pid: u32,
    current_pid: u32,
    kernel_probe: &dyn Fn(u32) -> KernelPidObservation,
) -> ProcessIdentityObservation {
    let domain = match read_identity_domain(proc_root, current_pid) {
        Ok(domain) => domain,
        Err(reason) => return ProcessIdentityObservation::Unknown(reason),
    };
    let stat = match std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return match kernel_probe(pid) {
                KernelPidObservation::Absent => ProcessIdentityObservation::Absent,
                KernelPidObservation::AccessDenied => {
                    ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::AccessDenied)
                }
                KernelPidObservation::Exists | KernelPidObservation::Unknown => {
                    ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed)
                }
            };
        }
        Err(error) => return io_failure(error),
    };
    let Some((state, start_time)) = parse_process_stat(&stat) else {
        return ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed);
    };
    let identity = ProcessInstanceIdentity::new(
        pid,
        format!(
            "linux:v1:{}:{}:{start_time}",
            domain.boot_identity, domain.pid_namespace
        ),
    );
    if matches!(state, 'Z' | 'X' | 'x') {
        ProcessIdentityObservation::Terminated(identity)
    } else {
        ProcessIdentityObservation::Alive(identity)
    }
}

fn io_failure(error: std::io::Error) -> ProcessIdentityObservation {
    let reason = if error.kind() == std::io::ErrorKind::PermissionDenied {
        ProcessIdentityUnknownReason::AccessDenied
    } else {
        ProcessIdentityUnknownReason::ProbeFailed
    };
    ProcessIdentityObservation::Unknown(reason)
}

fn parse_process_stat(stat: &str) -> Option<(char, u64)> {
    let comm_end = stat.rfind(')')?;
    let mut fields = stat.get(comm_end + 1..)?.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let start_time = fields.nth(18)?.parse().ok()?;
    Some((state, start_time))
}

fn validate_identity_domain_at(
    proc_root: &Path,
    original_birth_identity: &str,
    current_pid: u32,
) -> Result<(), ProcessIdentityUnknownReason> {
    let original = parse_linux_identity(original_birth_identity)?;
    let current = read_identity_domain(proc_root, current_pid)?;
    if original.boot_identity == current.boot_identity
        && original.pid_namespace == current.pid_namespace
    {
        Ok(())
    } else {
        Err(ProcessIdentityUnknownReason::IdentityDomainMismatch)
    }
}

fn parse_linux_identity(
    birth_identity: &str,
) -> Result<LinuxProcessIdentity, ProcessIdentityUnknownReason> {
    if !birth_identity.starts_with("linux:") {
        return Err(ProcessIdentityUnknownReason::IdentityDomainMismatch);
    }
    let fields = birth_identity.split(':').collect::<Vec<_>>();
    if fields.len() != 5 || fields[0] != "linux" || fields[1] != "v1" {
        return Err(ProcessIdentityUnknownReason::InvalidBirthIdentity);
    }
    let boot_identity = canonical_uuid(fields[2])
        .filter(|canonical| canonical == fields[2])
        .ok_or(ProcessIdentityUnknownReason::InvalidBirthIdentity)?;
    let pid_namespace = parse_canonical_positive_u64(fields[3])?;
    parse_canonical_positive_u64(fields[4])?;
    Ok(LinuxProcessIdentity {
        boot_identity,
        pid_namespace,
    })
}

fn read_identity_domain(
    proc_root: &Path,
    current_pid: u32,
) -> Result<LinuxIdentityDomain, ProcessIdentityUnknownReason> {
    let boot_identity =
        std::fs::read_to_string(proc_root.join("sys/kernel/random/boot_id")).map_err(io_reason)?;
    let boot_identity = canonical_uuid(boot_identity.trim())
        .ok_or(ProcessIdentityUnknownReason::IdentityDomainUnverified)?;
    let self_target = std::fs::read_link(proc_root.join("self")).map_err(io_reason)?;
    self_target
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|visible_pid| *visible_pid == current_pid)
        .ok_or(ProcessIdentityUnknownReason::IdentityDomainUnverified)?;
    let namespace_target = std::fs::read_link(proc_root.join("self/ns/pid")).map_err(io_reason)?;
    let pid_namespace = parse_pid_namespace(&namespace_target)
        .ok_or(ProcessIdentityUnknownReason::IdentityDomainUnverified)?;
    Ok(LinuxIdentityDomain {
        boot_identity,
        pid_namespace,
    })
}

fn canonical_uuid(value: &str) -> Option<String> {
    if value.len() != 36 {
        return None;
    }
    for (index, byte) in value.bytes().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
        } else if !byte.is_ascii_hexdigit() {
            return None;
        }
    }
    let canonical = value.to_ascii_lowercase();
    if canonical.bytes().all(|byte| byte == b'0' || byte == b'-') {
        None
    } else {
        Some(canonical)
    }
}

fn parse_pid_namespace(target: &Path) -> Option<u64> {
    let value = target.to_str()?;
    parse_canonical_positive_u64(value.strip_prefix("pid:[")?.strip_suffix(']')?).ok()
}

fn parse_canonical_positive_u64(value: &str) -> Result<u64, ProcessIdentityUnknownReason> {
    value
        .parse::<u64>()
        .ok()
        .filter(|parsed| *parsed > 0 && parsed.to_string() == value)
        .ok_or(ProcessIdentityUnknownReason::InvalidBirthIdentity)
}

fn kernel_pid_observation(pid: u32) -> KernelPidObservation {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return KernelPidObservation::Unknown;
    };
    match rustix::process::test_kill_process(pid) {
        Ok(()) => KernelPidObservation::Exists,
        Err(error) if error == rustix::io::Errno::PERM => KernelPidObservation::AccessDenied,
        Err(error) if error == rustix::io::Errno::SRCH => KernelPidObservation::Absent,
        Err(_) => KernelPidObservation::Unknown,
    }
}

fn io_reason(error: std::io::Error) -> ProcessIdentityUnknownReason {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ProcessIdentityUnknownReason::AccessDenied
    } else {
        ProcessIdentityUnknownReason::IdentityDomainUnverified
    }
}

struct LinuxIdentityDomain {
    boot_identity: String,
    pid_namespace: u64,
}

struct LinuxProcessIdentity {
    boot_identity: String,
    pid_namespace: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KernelPidObservation {
    Exists,
    AccessDenied,
    Absent,
    Unknown,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        KernelPidObservation, inspect_at, inspect_at_with_probe, io_failure, parse_process_stat,
        validate_identity_domain_at,
    };
    use super::{ProcessIdentityObservation, ProcessIdentityUnknownReason};

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn parses_start_time_after_comm_with_spaces_and_parentheses() {
        let intermediate = vec!["0"; 18].join(" ");
        let stat = format!("123 (worker ) name) S {intermediate} 98765");

        assert_eq!(parse_process_stat(&stat), Some(('S', 98765)));
    }

    #[test]
    fn missing_proc_mount_is_unknown() {
        let root = TempRoot::new();

        let observation = inspect_at(&root.path().join("missing"), 999_999);

        assert!(matches!(
            observation,
            ProcessIdentityObservation::Unknown(
                ProcessIdentityUnknownReason::IdentityDomainUnverified
            )
        ));
    }

    #[test]
    fn hidden_live_pid_is_unknown() {
        let root = TempRoot::new();
        root.write_domain("7f27acbd-5331-4b08-a56f-3c580f430912", 4026531836);

        let observation = inspect_at_with_probe(root.path(), 999_999, std::process::id(), &|_| {
            KernelPidObservation::Exists
        });

        assert!(matches!(
            observation,
            ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed)
        ));
    }

    #[test]
    fn qualified_domain_with_esrch_is_absent() {
        let root = TempRoot::new();
        root.write_domain("7f27acbd-5331-4b08-a56f-3c580f430912", 4026531836);

        let observation = inspect_at_with_probe(root.path(), 999_999, std::process::id(), &|_| {
            KernelPidObservation::Absent
        });

        assert!(matches!(observation, ProcessIdentityObservation::Absent));
    }

    #[test]
    fn inaccessible_hidden_pid_is_unknown() {
        let root = TempRoot::new();
        root.write_domain("7f27acbd-5331-4b08-a56f-3c580f430912", 4026531836);

        let observation = inspect_at_with_probe(root.path(), 999_999, std::process::id(), &|_| {
            KernelPidObservation::AccessDenied
        });

        assert!(matches!(
            observation,
            ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::AccessDenied)
        ));
    }

    #[test]
    fn platform_boot_and_pid_namespace_mismatches_are_rejected() {
        let root = TempRoot::new();
        root.write_domain("7f27acbd-5331-4b08-a56f-3c580f430912", 4026531836);
        let current_pid = std::process::id();

        for identity in [
            "windows:v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:0000000000000001",
            "linux:v1:8f27acbd-5331-4b08-a56f-3c580f430912:4026531836:98765",
            "linux:v1:7f27acbd-5331-4b08-a56f-3c580f430912:4026531837:98765",
        ] {
            assert_eq!(
                validate_identity_domain_at(root.path(), identity, current_pid),
                Err(ProcessIdentityUnknownReason::IdentityDomainMismatch)
            );
        }
    }

    #[test]
    fn matching_linux_identity_domain_is_valid() {
        let root = TempRoot::new();
        root.write_domain("7f27acbd-5331-4b08-a56f-3c580f430912", 4026531836);

        let result = validate_identity_domain_at(
            root.path(),
            "linux:v1:7f27acbd-5331-4b08-a56f-3c580f430912:4026531836:98765",
            std::process::id(),
        );

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn zombie_observation_keeps_its_process_identity() {
        let root = TempRoot::new();
        root.write_domain("7f27acbd-5331-4b08-a56f-3c580f430912", 4026531836);
        root.write_stat(123, 'Z', 98765);

        let observation = inspect_at_with_probe(root.path(), 123, std::process::id(), &|_| {
            panic!("existing stat must not use kernel fallback")
        });

        assert!(matches!(
            observation,
            ProcessIdentityObservation::Terminated(ref identity)
                if identity.pid() == 123
                    && identity.birth_identity()
                        == "linux:v1:7f27acbd-5331-4b08-a56f-3c580f430912:4026531836:98765"
        ));
    }

    #[test]
    fn access_denied_proc_read_is_unknown() {
        let observation = io_failure(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

        assert!(matches!(
            observation,
            ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::AccessDenied)
        ));
    }

    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new() -> Self {
            let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("julie-reader-liveness-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write_domain(&self, boot_id: &str, pid_namespace: u64) {
            let directory = self.path.join("sys/kernel/random");
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("boot_id"), boot_id).unwrap();
            let current_pid = std::process::id();
            let namespace_directory = self.path.join(current_pid.to_string()).join("ns");
            fs::create_dir_all(&namespace_directory).unwrap();
            std::os::unix::fs::symlink(current_pid.to_string(), self.path.join("self")).unwrap();
            std::os::unix::fs::symlink(
                format!("pid:[{pid_namespace}]"),
                namespace_directory.join("pid"),
            )
            .unwrap();
        }

        fn write_stat(&self, pid: u32, state: char, start_time: u64) {
            let directory = self.path.join(pid.to_string());
            fs::create_dir_all(&directory).unwrap();
            let intermediate = vec!["0"; 18].join(" ");
            fs::write(
                directory.join("stat"),
                format!("{pid} (worker ) name) {state} {intermediate} {start_time}"),
            )
            .unwrap();
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}
