mod connection;
mod coordinator;
mod generation;
mod layout;
mod log;
mod maintenance;
mod manifest;
mod model;
mod pragmas;
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
    ExecutionContext, ExecutionQuantum, IntentIdentity, LeaseDisposition, LeaseHolder,
    MaintenanceOwnerFence, PidLiveness, PidStatus, QUANTUM_OVERRUN_CODE, RequestKind,
    RequestReceipt, RequestState, StoreCoordinator, UnixMillisClock, compare_versions,
    foreign_live_maintenance_intent, process_status,
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
    CapacityPlan, CapacityProvider, DeltaVersionFact, MaintenanceApplyPolicy,
    MaintenanceApplyReport, MaintenanceCapacity, MaintenanceClock, MaintenanceError,
    MaintenanceExecutor, MaintenanceInspector, MaintenanceLevel, MaintenancePlan,
    MaintenancePolicy, MaintenanceRootKind, MaintenanceRun, MaintenanceSnapshot, ManifestFact,
    ManifestVersionFact, PlanBinding, RetentionPlan, RetireViewApplied, RetireViewPlan,
    VersionFact, plan_maintenance, plan_view_retirement,
};
pub use manifest::{
    MANIFEST_HASH_ALGORITHM, MANIFEST_PUBLISH_MAX_RETRIES, ManifestBuilder, ManifestEntry,
    ManifestEntryStatus, ManifestPublishDisposition, ManifestPublishResult, ManifestStore,
    ManifestStoreError, ViewEnsureDisposition, same_path_identity,
};
pub use model::{
    FamilyAllocatorKind, GenerationState, MaintenanceAction, StoreFileVersion, StoreLevel,
    StoreProjectionError,
};
pub(crate) use model::{StoreReferenceSite, StoreRowCounts};
pub use schema::{
    STORE_FORMAT_EPOCH, STORE_SQLITE_SCHEMA_VERSION, StoreSchemaError, create_coordinator_schema,
    create_store_schema,
};
pub use writer::{StoreVersionState, StoreWriteRequest, StoreWriter, StoreWriterError};
