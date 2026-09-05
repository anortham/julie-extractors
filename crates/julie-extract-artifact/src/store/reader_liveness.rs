use std::fmt;

#[cfg(target_os = "linux")]
#[path = "reader_liveness/linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "reader_liveness/windows.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use super::{ProcessIdentityObservation, ProcessIdentityUnknownReason};

    pub(super) fn inspect(_pid: u32) -> ProcessIdentityObservation {
        ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::UnsupportedPlatform)
    }

    pub(super) fn validate_identity_domain(
        _original_birth_identity: &str,
    ) -> Result<(), ProcessIdentityUnknownReason> {
        Err(ProcessIdentityUnknownReason::UnsupportedPlatform)
    }
}

const READER_IDENTITY_UNKNOWN_WARNING: &str = "reader_identity_unknown";

#[derive(Clone, PartialEq, Eq)]
pub struct ProcessInstanceIdentity {
    pid: u32,
    birth_identity: String,
}

impl ProcessInstanceIdentity {
    pub fn new(pid: u32, birth_identity: impl Into<String>) -> Self {
        Self {
            pid,
            birth_identity: birth_identity.into(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn birth_identity(&self) -> &str {
        &self.birth_identity
    }
}

impl fmt::Debug for ProcessInstanceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessInstanceIdentity")
            .field("pid", &self.pid)
            .field("birth_identity", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessIdentityUnknownReason {
    MissingBirthIdentity,
    InvalidBirthIdentity,
    ProcessInstanceMismatch,
    IdentityDomainUnverified,
    IdentityDomainMismatch,
    AccessDenied,
    ProbeFailed,
    UnsupportedPlatform,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessIdentityObservation {
    Alive(ProcessInstanceIdentity),
    Terminated(ProcessInstanceIdentity),
    Absent,
    Unknown(ProcessIdentityUnknownReason),
}

pub trait ProcessIdentityProbe: Send + Sync {
    fn inspect(&self, pid: u32) -> ProcessIdentityObservation;

    fn validate_identity_domain(
        &self,
        _original_birth_identity: &str,
    ) -> Result<(), ProcessIdentityUnknownReason> {
        Err(ProcessIdentityUnknownReason::IdentityDomainUnverified)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcessIdentityProbe;

impl ProcessIdentityProbe for SystemProcessIdentityProbe {
    fn inspect(&self, pid: u32) -> ProcessIdentityObservation {
        platform::inspect(pid)
    }

    fn validate_identity_domain(
        &self,
        original_birth_identity: &str,
    ) -> Result<(), ProcessIdentityUnknownReason> {
        platform::validate_identity_domain(original_birth_identity)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathQualification {
    RetainedUnexpired,
    RetainedAlive,
    DefinitivelyDead,
    RetainedUnknown(ProcessIdentityUnknownReason),
}

impl DeathQualification {
    pub fn warning_code(self) -> Option<&'static str> {
        match self {
            Self::RetainedUnknown(_) => Some(READER_IDENTITY_UNKNOWN_WARNING),
            _ => None,
        }
    }
}

pub fn qualify_reader_owner(
    owner_pid: u32,
    owner_birth_identity: &str,
    expires_at: i64,
    now_ms: i64,
    probe: &dyn ProcessIdentityProbe,
) -> DeathQualification {
    if now_ms < expires_at {
        return DeathQualification::RetainedUnexpired;
    }
    if owner_birth_identity.is_empty() {
        return DeathQualification::RetainedUnknown(
            ProcessIdentityUnknownReason::MissingBirthIdentity,
        );
    }
    if owner_birth_identity.len() > 512
        || owner_birth_identity.trim() != owner_birth_identity
        || owner_birth_identity.chars().any(char::is_control)
    {
        return DeathQualification::RetainedUnknown(
            ProcessIdentityUnknownReason::InvalidBirthIdentity,
        );
    }
    match probe.inspect(owner_pid) {
        ProcessIdentityObservation::Alive(identity)
            if identity.pid() == owner_pid && identity.birth_identity() == owner_birth_identity =>
        {
            DeathQualification::RetainedAlive
        }
        ProcessIdentityObservation::Alive(_) => DeathQualification::RetainedUnknown(
            ProcessIdentityUnknownReason::ProcessInstanceMismatch,
        ),
        ProcessIdentityObservation::Terminated(identity)
            if identity.pid() == owner_pid && identity.birth_identity() == owner_birth_identity =>
        {
            qualify_dead_identity_domain(probe, owner_birth_identity)
        }
        ProcessIdentityObservation::Terminated(_) => DeathQualification::RetainedUnknown(
            ProcessIdentityUnknownReason::ProcessInstanceMismatch,
        ),
        ProcessIdentityObservation::Absent => {
            qualify_dead_identity_domain(probe, owner_birth_identity)
        }
        ProcessIdentityObservation::Unknown(reason) => DeathQualification::RetainedUnknown(reason),
    }
}

fn qualify_dead_identity_domain(
    probe: &dyn ProcessIdentityProbe,
    owner_birth_identity: &str,
) -> DeathQualification {
    match probe.validate_identity_domain(owner_birth_identity) {
        Ok(()) => DeathQualification::DefinitivelyDead,
        Err(reason) => DeathQualification::RetainedUnknown(reason),
    }
}
