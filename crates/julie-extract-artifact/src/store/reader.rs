use sha2::{Digest, Sha256};

pub const READER_MIN_WRITER_VERSION: &str = "2.40.0";

pub struct ReaderAcquireRequest {
    family_id: String,
    view_id: String,
    generation_name: String,
    owner_label: String,
    owner_pid: u32,
    owner_nonce: String,
    lease_ms: u64,
}

impl ReaderAcquireRequest {
    pub fn new(
        family_id: impl Into<String>,
        view_id: impl Into<String>,
        generation_name: impl Into<String>,
        owner_label: impl Into<String>,
        owner_pid: u32,
        owner_nonce: impl Into<String>,
        lease_ms: u64,
    ) -> Self {
        Self {
            family_id: family_id.into(),
            view_id: view_id.into(),
            generation_name: generation_name.into(),
            owner_label: owner_label.into(),
            owner_pid,
            owner_nonce: owner_nonce.into(),
            lease_ms,
        }
    }

    pub fn family_id(&self) -> &str {
        &self.family_id
    }

    pub fn view_id(&self) -> &str {
        &self.view_id
    }

    pub fn generation_name(&self) -> &str {
        &self.generation_name
    }

    pub fn owner_label(&self) -> &str {
        &self.owner_label
    }

    pub fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    pub fn owner_nonce(&self) -> &str {
        &self.owner_nonce
    }

    pub fn lease_ms(&self) -> u64 {
        self.lease_ms
    }
}

pub struct ReaderRenewRequest {
    family_id: String,
    pin_id: String,
    owner_nonce: String,
    owner_pid: u32,
    lease_ms: u64,
}

impl ReaderRenewRequest {
    pub fn new(
        family_id: impl Into<String>,
        pin_id: impl Into<String>,
        owner_nonce: impl Into<String>,
        owner_pid: u32,
        lease_ms: u64,
    ) -> Self {
        Self {
            family_id: family_id.into(),
            pin_id: pin_id.into(),
            owner_nonce: owner_nonce.into(),
            owner_pid,
            lease_ms,
        }
    }

    pub fn family_id(&self) -> &str {
        &self.family_id
    }

    pub fn pin_id(&self) -> &str {
        &self.pin_id
    }

    pub fn owner_nonce(&self) -> &str {
        &self.owner_nonce
    }

    pub fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    pub fn lease_ms(&self) -> u64 {
        self.lease_ms
    }
}

pub struct ReaderReleaseRequest {
    family_id: String,
    pin_id: String,
    owner_nonce: String,
}

impl ReaderReleaseRequest {
    pub fn new(
        family_id: impl Into<String>,
        pin_id: impl Into<String>,
        owner_nonce: impl Into<String>,
    ) -> Self {
        Self {
            family_id: family_id.into(),
            pin_id: pin_id.into(),
            owner_nonce: owner_nonce.into(),
        }
    }

    pub fn family_id(&self) -> &str {
        &self.family_id
    }

    pub fn pin_id(&self) -> &str {
        &self.pin_id
    }

    pub fn owner_nonce(&self) -> &str {
        &self.owner_nonce
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReaderOwnerIdentity {
    pin_id: String,
    owner_nonce: String,
    owner_label: String,
    owner_pid: u32,
    owner_birth_identity: String,
}

impl ReaderOwnerIdentity {
    pub(crate) fn new(
        pin_id: impl Into<String>,
        owner_nonce: impl Into<String>,
        owner_label: impl Into<String>,
        owner_pid: u32,
        owner_birth_identity: impl Into<String>,
    ) -> Self {
        Self {
            pin_id: pin_id.into(),
            owner_nonce: owner_nonce.into(),
            owner_label: owner_label.into(),
            owner_pid,
            owner_birth_identity: owner_birth_identity.into(),
        }
    }

    pub fn pin_id(&self) -> &str {
        &self.pin_id
    }

    pub fn owner_nonce(&self) -> &str {
        &self.owner_nonce
    }

    pub fn owner_label(&self) -> &str {
        &self.owner_label
    }

    pub fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    pub(crate) fn owner_birth_identity(&self) -> &str {
        &self.owner_birth_identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderManifestSnapshot {
    family_id: String,
    view_id: String,
    manifest_generation: i64,
    generation_name: String,
    store_instance_id: String,
    manifest_hash: String,
    extraction_identity_epoch: i64,
    served_store_log_sequence: i64,
    min_retained_store_log_sequence: i64,
    snapshot_fingerprint: String,
}

impl ReaderManifestSnapshot {
    #[allow(
        clippy::too_many_arguments,
        reason = "the public immutable snapshot constructor mirrors the frozen reader contract"
    )]
    pub fn new(
        family_id: impl Into<String>,
        view_id: impl Into<String>,
        manifest_generation: i64,
        generation_name: impl Into<String>,
        manifest_hash: impl Into<String>,
        extraction_identity_epoch: i64,
        served_store_log_sequence: i64,
        min_retained_store_log_sequence: i64,
    ) -> Self {
        let family_id = family_id.into();
        let view_id = view_id.into();
        let generation_name = generation_name.into();
        let store_instance_id = format!("{family_id}:{generation_name}");
        let manifest_hash = manifest_hash.into();
        let mut snapshot = Self {
            family_id,
            view_id,
            manifest_generation,
            generation_name,
            store_instance_id,
            manifest_hash,
            extraction_identity_epoch,
            served_store_log_sequence,
            min_retained_store_log_sequence,
            snapshot_fingerprint: String::new(),
        };
        snapshot.snapshot_fingerprint = snapshot_fingerprint(&snapshot);
        snapshot
    }

    pub fn family_id(&self) -> &str {
        &self.family_id
    }

    pub fn view_id(&self) -> &str {
        &self.view_id
    }

    pub fn manifest_generation(&self) -> i64 {
        self.manifest_generation
    }

    pub fn generation_name(&self) -> &str {
        &self.generation_name
    }

    pub fn store_instance_id(&self) -> &str {
        &self.store_instance_id
    }

    pub fn manifest_hash(&self) -> &str {
        &self.manifest_hash
    }

    pub fn extraction_identity_epoch(&self) -> i64 {
        self.extraction_identity_epoch
    }

    pub fn served_store_log_sequence(&self) -> i64 {
        self.served_store_log_sequence
    }

    pub fn min_retained_store_log_sequence(&self) -> i64 {
        self.min_retained_store_log_sequence
    }

    pub fn snapshot_fingerprint(&self) -> &str {
        &self.snapshot_fingerprint
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReaderRegistration {
    identity: ReaderOwnerIdentity,
    snapshot: ReaderManifestSnapshot,
    acquired_at: i64,
    heartbeat_at: i64,
    expires_at: i64,
}

impl ReaderRegistration {
    pub(crate) fn new(
        identity: ReaderOwnerIdentity,
        snapshot: ReaderManifestSnapshot,
        acquired_at: i64,
        heartbeat_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            identity,
            snapshot,
            acquired_at,
            heartbeat_at,
            expires_at,
        }
    }

    pub fn identity(&self) -> &ReaderOwnerIdentity {
        &self.identity
    }

    pub fn snapshot(&self) -> &ReaderManifestSnapshot {
        &self.snapshot
    }

    pub fn acquired_at(&self) -> i64 {
        self.acquired_at
    }

    pub fn heartbeat_at(&self) -> i64 {
        self.heartbeat_at
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

pub struct ReaderAcquireResult {
    registration: ReaderRegistration,
}

impl ReaderAcquireResult {
    #[allow(dead_code)]
    pub(crate) fn new(registration: ReaderRegistration) -> Self {
        Self { registration }
    }

    pub fn registration(&self) -> &ReaderRegistration {
        &self.registration
    }

    pub fn into_registration(self) -> ReaderRegistration {
        self.registration
    }
}

pub struct ReaderReportFacts {
    pin_id: String,
    owner_nonce: String,
    owner_pid: u32,
    snapshot: ReaderManifestSnapshot,
    expires_at: i64,
    warning: Option<String>,
}

impl ReaderReportFacts {
    pub fn from_registration(registration: &ReaderRegistration, warning: Option<String>) -> Self {
        Self {
            pin_id: registration.identity.pin_id.clone(),
            owner_nonce: registration.identity.owner_nonce.clone(),
            owner_pid: registration.identity.owner_pid,
            snapshot: registration.snapshot.clone(),
            expires_at: registration.expires_at,
            warning,
        }
    }

    pub fn pin_id(&self) -> &str {
        &self.pin_id
    }

    pub fn owner_nonce(&self) -> &str {
        &self.owner_nonce
    }

    pub fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    pub fn snapshot(&self) -> &ReaderManifestSnapshot {
        &self.snapshot
    }

    pub fn protected_manifest_count(&self) -> usize {
        1
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

fn snapshot_fingerprint(snapshot: &ReaderManifestSnapshot) -> String {
    let mut digest = Sha256::new();
    digest.update(b"julie-reader-snapshot-v1\0");
    for value in [
        snapshot.family_id.as_str(),
        snapshot.store_instance_id.as_str(),
        snapshot.view_id.as_str(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.update(snapshot.manifest_generation.to_be_bytes());
    for value in [
        snapshot.manifest_hash.as_str(),
        snapshot.generation_name.as_str(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    for value in [
        snapshot.extraction_identity_epoch,
        snapshot.served_store_log_sequence,
        snapshot.min_retained_store_log_sequence,
    ] {
        digest.update(value.to_be_bytes());
    }
    format!("{:x}", digest.finalize())
}
