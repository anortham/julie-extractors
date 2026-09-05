mod connection;
mod coordinator;
mod generation;
mod layout;
mod log;
mod maintenance;
mod manifest;
mod model;
mod pragmas;
mod reader;
mod reader_liveness;
mod rows;
mod schema;
#[cfg(feature = "test-store-crash")]
#[doc(hidden)]
pub mod test_hooks;
mod wal_retry;
mod writer;

pub use connection::{
    GenerationFence, StoreConnectionError, StoreConnectionFactory, StoreWriterConnection,
};
pub use coordinator::{
    ConsumerCursor, CoordinatorError, CoordinatorExecutor, CoordinatorPolicy, CoordinatorRequest,
    DrainReport, EnqueueResult, ExecutionContext, ExecutionQuantum, IntentIdentity,
    LeaseDisposition, LeaseHolder, MaintenanceOwnerFence, PidLiveness, PidStatus,
    QUANTUM_OVERRUN_CODE, ReconcileOutcome, RequestKind, RequestReceipt, RequestState,
    StoreCoordinator, UnixMillisClock, compare_versions, foreign_live_maintenance_intent,
    process_status,
};
pub use generation::{
    GenerationApplyReport, GenerationError, GenerationLifecycle, GenerationPolicy,
    RepairDisposition,
};
pub use layout::{
    PartialGenerationOwner, StoreLayout, StoreLayoutError, write_partial_generation_owner,
};
pub use log::{StoreLog, StoreLogEntry, StoreLogError};
pub use maintenance::{
    AllocatorMark, BaseVersionFact, CapacityPlan, CapacityProvider, ConsumerCursorFact,
    CoordinatorRequestFact, DeltaVersionFact, DemotionCandidate, FailedPathFact,
    MaintenanceApplyPolicy, MaintenanceApplyReport, MaintenanceCapacity, MaintenanceClock,
    MaintenanceError, MaintenanceExecutor, MaintenanceInspector, MaintenanceLevel, MaintenancePlan,
    MaintenancePolicy, MaintenanceRootKind, MaintenanceRun, MaintenanceSnapshot, ManifestFact,
    ManifestVersionFact, PlanBinding, ProtectionReason, ReaderMaintenanceDisposition,
    ReaderMaintenanceRoot, RetentionPlan, RetireViewApplied, RetireViewPlan, VersionDecision,
    VersionFact, VersionRootFact, plan_maintenance, plan_view_retirement,
};
pub use manifest::{
    MANIFEST_HASH_ALGORITHM, MANIFEST_PUBLISH_MAX_RETRIES, ManifestBuilder, ManifestEntry,
    ManifestEntryStatus, ManifestPublishDisposition, ManifestPublishResult, ManifestStore,
    ManifestStoreError, ViewEnsureDisposition, same_path_identity,
};
pub use model::{
    FamilyAllocatorKind, GenerationState, MaintenanceAction, StoreFileVersion, StoreLevel,
    StoreProjectionError, StoreReferenceSite, StoreRowCounts,
};
pub use reader::{
    READER_MIN_WRITER_VERSION, ReaderAcquireRequest, ReaderAcquireResult, ReaderManifestSnapshot,
    ReaderOwnerIdentity, ReaderRegistration, ReaderReleaseRequest, ReaderRenewRequest,
    ReaderReportFacts,
};
pub use reader_liveness::{
    DeathQualification, ProcessIdentityObservation, ProcessIdentityProbe,
    ProcessIdentityUnknownReason, ProcessInstanceIdentity, SystemProcessIdentityProbe,
    qualify_reader_owner,
};
pub use schema::{
    STORE_FORMAT_EPOCH, STORE_SQLITE_SCHEMA_VERSION, StoreSchemaError, create_coordinator_schema,
    create_store_schema,
};
pub use writer::{
    StoreVersionState, StoreWriteRequest, StoreWriteResult, StoreWriter, StoreWriterError,
    StoredFileVersion,
};
