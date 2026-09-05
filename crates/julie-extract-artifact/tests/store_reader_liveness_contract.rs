#[cfg(any(target_os = "linux", windows))]
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use julie_extract_artifact::store::{
    DeathQualification, ProcessIdentityObservation, ProcessIdentityProbe,
    ProcessIdentityUnknownReason, ProcessInstanceIdentity, SystemProcessIdentityProbe,
    qualify_reader_owner,
};

struct FakeProbe {
    observation: fn(u32) -> ProcessIdentityObservation,
    domain_validation: Result<(), ProcessIdentityUnknownReason>,
    calls: AtomicUsize,
}

impl FakeProbe {
    fn new(
        observation: fn(u32) -> ProcessIdentityObservation,
        domain_validation: Result<(), ProcessIdentityUnknownReason>,
    ) -> Self {
        Self {
            observation,
            domain_validation,
            calls: AtomicUsize::new(0),
        }
    }
}

impl ProcessIdentityProbe for FakeProbe {
    fn inspect(&self, pid: u32) -> ProcessIdentityObservation {
        self.calls.fetch_add(1, Ordering::Relaxed);
        (self.observation)(pid)
    }

    fn validate_identity_domain(
        &self,
        _original_birth_identity: &str,
    ) -> Result<(), ProcessIdentityUnknownReason> {
        self.domain_validation
    }
}

#[cfg(any(target_os = "linux", windows))]
struct LivenessChild {
    child: Child,
    waited: bool,
}

#[cfg(any(target_os = "linux", windows))]
impl LivenessChild {
    fn spawn() -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "liveness_child_process",
                "--ignored",
                "--nocapture",
            ])
            .env("JULIE_READER_LIVENESS_CHILD", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            child,
            waited: false,
        }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn terminate_and_wait(&mut self) {
        if self.waited {
            return;
        }
        if let Err(error) = self.child.kill() {
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
        self.child.wait().unwrap();
        self.waited = true;
    }
}

#[cfg(any(target_os = "linux", windows))]
impl Drop for LivenessChild {
    fn drop(&mut self) {
        if !self.waited {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.waited = true;
        }
    }
}

#[test]
fn unexpired_reader_is_retained_without_a_probe() {
    let probe = FakeProbe::new(
        |_| panic!("unexpired reader must not be probed"),
        Err(ProcessIdentityUnknownReason::IdentityDomainUnverified),
    );

    let result = qualify_reader_owner(41, "birth-a", 101, 100, &probe);

    assert!(matches!(result, DeathQualification::RetainedUnexpired));
    assert_eq!(probe.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn expired_paused_reader_is_retained() {
    let probe = FakeProbe::new(
        |pid| ProcessIdentityObservation::Alive(ProcessInstanceIdentity::new(pid, "birth-a")),
        Ok(()),
    );

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(result, DeathQualification::RetainedAlive));
}

#[test]
fn expired_absent_reader_is_definitively_dead() {
    let probe = FakeProbe::new(|_| ProcessIdentityObservation::Absent, Ok(()));

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(result, DeathQualification::DefinitivelyDead));
}

#[test]
fn pid_reuse_is_unknown() {
    let probe = FakeProbe::new(
        |pid| ProcessIdentityObservation::Alive(ProcessInstanceIdentity::new(pid, "birth-b")),
        Ok(()),
    );

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(
        result,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::ProcessInstanceMismatch)
    ));
    assert_eq!(result.warning_code(), Some("reader_identity_unknown"));
}

#[test]
fn access_denial_is_unknown_not_absence() {
    let probe = FakeProbe::new(
        |_| ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::AccessDenied),
        Ok(()),
    );

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(
        result,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::AccessDenied)
    ));
    assert!(!matches!(result, DeathQualification::DefinitivelyDead));
}

#[test]
fn probe_failure_and_unsupported_platform_are_unknown() {
    for reason in [
        ProcessIdentityUnknownReason::ProbeFailed,
        ProcessIdentityUnknownReason::UnsupportedPlatform,
    ] {
        let probe = FakeProbe::new(move_reason(reason), Ok(()));
        let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);
        assert!(matches!(result, DeathQualification::RetainedUnknown(found) if found == reason));
    }
}

fn move_reason(reason: ProcessIdentityUnknownReason) -> fn(u32) -> ProcessIdentityObservation {
    match reason {
        ProcessIdentityUnknownReason::ProbeFailed => probe_failed,
        ProcessIdentityUnknownReason::UnsupportedPlatform => unsupported,
        _ => unreachable!(),
    }
}

fn probe_failed(_: u32) -> ProcessIdentityObservation {
    ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed)
}

fn unsupported(_: u32) -> ProcessIdentityObservation {
    ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::UnsupportedPlatform)
}

#[test]
fn missing_and_invalid_birth_identity_are_unknown_without_a_probe() {
    let probe = FakeProbe::new(
        |_| panic!("invalid identity must not be probed"),
        Err(ProcessIdentityUnknownReason::IdentityDomainUnverified),
    );

    let missing = qualify_reader_owner(41, "", 100, 100, &probe);
    let invalid = qualify_reader_owner(41, " birth-a", 100, 100, &probe);

    assert!(matches!(
        missing,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::MissingBirthIdentity)
    ));
    assert!(matches!(
        invalid,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::InvalidBirthIdentity)
    ));
    assert_eq!(probe.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn identity_debug_output_redacts_the_birth_identity() {
    let identity = ProcessInstanceIdentity::new(41, "secret-birth-token");

    let output = format!("{identity:?}");

    assert!(output.contains("41"));
    assert!(!output.contains("secret-birth-token"));
}

#[test]
fn absent_without_domain_proof_is_unknown() {
    struct DefaultFailClosedProbe;

    impl ProcessIdentityProbe for DefaultFailClosedProbe {
        fn inspect(&self, _pid: u32) -> ProcessIdentityObservation {
            ProcessIdentityObservation::Absent
        }
    }

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &DefaultFailClosedProbe);

    assert!(matches!(
        result,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::IdentityDomainUnverified)
    ));
}

#[test]
fn absent_with_mismatched_domain_is_unknown() {
    let probe = FakeProbe::new(
        |_| ProcessIdentityObservation::Absent,
        Err(ProcessIdentityUnknownReason::IdentityDomainMismatch),
    );

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(
        result,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::IdentityDomainMismatch)
    ));
}

#[test]
fn terminated_matching_reader_is_definitively_dead() {
    let probe = FakeProbe::new(
        |pid| ProcessIdentityObservation::Terminated(ProcessInstanceIdentity::new(pid, "birth-a")),
        Ok(()),
    );

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(result, DeathQualification::DefinitivelyDead));
}

#[test]
fn terminated_reused_pid_is_unknown() {
    let probe = FakeProbe::new(
        |pid| ProcessIdentityObservation::Terminated(ProcessInstanceIdentity::new(pid, "birth-b")),
        Ok(()),
    );

    let result = qualify_reader_owner(41, "birth-a", 100, 100, &probe);

    assert!(matches!(
        result,
        DeathQualification::RetainedUnknown(ProcessIdentityUnknownReason::ProcessInstanceMismatch)
    ));
}

#[cfg(any(target_os = "linux", windows))]
#[test]
fn liveness_child_is_reaped_during_unwind() {
    let mut pid = 0;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let child = LivenessChild::spawn();
        pid = child.id();
        panic!("exercise panic cleanup");
    }));

    assert!(result.is_err());
    assert!(matches!(
        SystemProcessIdentityProbe.inspect(pid),
        ProcessIdentityObservation::Absent
    ));
}

#[cfg(any(target_os = "linux", windows))]
#[test]
fn system_probe_observes_process_instance_and_definitive_exit() {
    let mut child = LivenessChild::spawn();
    let pid = child.id();
    let probe = SystemProcessIdentityProbe;
    let identity = match probe.inspect(pid) {
        ProcessIdentityObservation::Alive(identity) => identity,
        _ => panic!("new child process identity was not observable"),
    };

    assert_eq!(identity.pid(), pid);
    assert!(!identity.birth_identity().is_empty());
    assert!(matches!(
        qualify_reader_owner(pid, identity.birth_identity(), 0, 1, &probe),
        DeathQualification::RetainedAlive
    ));

    child.terminate_and_wait();

    #[cfg(target_os = "linux")]
    assert!(matches!(
        probe.inspect(pid),
        ProcessIdentityObservation::Absent
    ));
    #[cfg(windows)]
    assert!(matches!(
        probe.inspect(pid),
        ProcessIdentityObservation::Terminated(ref terminated)
            if terminated.pid() == identity.pid()
                && terminated.birth_identity() == identity.birth_identity()
    ));
    assert!(matches!(
        qualify_reader_owner(pid, identity.birth_identity(), 0, 1, &probe),
        DeathQualification::DefinitivelyDead
    ));
}

#[test]
#[ignore]
fn liveness_child_process() {
    if std::env::var_os("JULIE_READER_LIVENESS_CHILD").is_some() {
        loop {
            std::thread::park();
        }
    }
}
