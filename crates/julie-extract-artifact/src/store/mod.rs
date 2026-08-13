mod connection;
mod coordinator;
mod generation;
mod layout;
mod log;
mod maintenance;
mod manifest;
mod model;
mod pragmas;
mod resolution;
mod resolution_diff;
mod rows;
mod schema;
mod scope;
#[cfg(feature = "test-store-crash")]
#[doc(hidden)]
pub mod test_hooks;
mod writer;

pub use connection::{
    GenerationFence, StoreConnectionError, StoreConnectionFactory, StoreWriterConnection,
};
pub use coordinator::{
    ConsumerCursor, CoordinatorError, CoordinatorExecutor, CoordinatorPolicy, CoordinatorRequest,
    DrainReport, EnqueueResult, ExecutionContext, ExecutionQuantum, IntentIdentity,
    LeaseDisposition, LeaseHolder, LeaseRecord, MaintenanceOwnerFence, PidLiveness, PidStatus,
    ReconcileOutcome, RequestKind, RequestReceipt, RequestState, StoreCoordinator, UnixMillisClock,
    compare_versions, foreign_live_maintenance_intent, process_status,
};
pub use generation::{
    GenerationApplyReport, GenerationError, GenerationLifecycle, GenerationPolicy,
    RepairDisposition,
};
pub use layout::{
    PartialGenerationOwner, StoreLayout, StoreLayoutError, write_partial_generation_owner,
};
pub use log::{StoreLog, StoreLogEntry, StoreLogError, StoreLogRecord};
pub use maintenance::{
    AllocatorMark, BaseVersionFact, CapacityPlan, CapacityProvider, ConsumerCursorFact,
    CoordinatorRequestFact, DeltaVersionFact, DemotionCandidate, FailedPathFact,
    MaintenanceApplyPolicy, MaintenanceApplyReport, MaintenanceCapacity, MaintenanceClock,
    MaintenanceError, MaintenanceExecutor, MaintenanceInspector, MaintenanceLevel, MaintenancePlan,
    MaintenancePolicy, MaintenanceRootKind, MaintenanceRun, MaintenanceSnapshot, ManifestFact,
    ManifestVersionFact, PlanBinding, ProtectionReason, RetentionPlan, VersionDecision,
    VersionFact, VersionRootFact, plan_maintenance,
};
pub use manifest::{
    BuiltManifest, MANIFEST_HASH_ALGORITHM, MANIFEST_PUBLISH_MAX_RETRIES, ManifestBuilder,
    ManifestEntry, ManifestEntryStatus, ManifestPublishDisposition, ManifestPublishResult,
    ManifestStore, ManifestStoreError, ViewEnsureDisposition,
};
pub use model::{
    FamilyAllocatorKind, GenerationState, MaintenanceAction, ResolutionBaseRecord,
    ResolutionBaseState, ResolutionDeltaRecord, ResolutionIdentifierDeltaRecord,
    ResolutionPendingDeltaRecord, ResolutionPendingOperation, ResolutionPinOwnerKind,
    ResolutionPinRecord, StoreFileVersion, StoreLevel, StoreProjectionError, StoreReferenceSite,
    StoreRowCounts, ViewResolutionState,
};
pub use resolution::{
    IdentifierResolutionRow, PendingResolutionRow, RESOLUTION_BASE_FORMAT_VERSION,
    RESOLUTION_BASE_SQL, RESOLUTION_BASE_USER_VERSION, ResolutionBaseBegin, ResolutionBaseBuild,
    ResolutionBaseBuilder, ResolutionBaseCatalog, ResolutionBaseCatalogError, ResolutionBaseReader,
    ResolutionBaseRecovery, ResolutionBaseWriter, ResolutionBindingError, ResolutionBindingStore,
    ResolutionConvergenceBegin, ResolutionExactPublish, ResolutionFileIdentity,
    ResolutionIdentifierRow, ResolutionPendingRow, ResolutionPublicationFence,
    ResolutionPublicationMarker, ResolutionSemanticCounts, ResolutionValidationError,
    ResolutionViewBinding, create_resolution_scratch_connection, resolution_base_catalog_hash,
    resolution_base_catalog_hash_for_sql, resolution_base_id,
};
pub use resolution_diff::{
    RESOLUTION_SCRATCH_FORMAT_VERSION, RESOLUTION_SCRATCH_SQL, RESOLUTION_SCRATCH_USER_VERSION,
    ResolutionApplyCounts, ResolutionDiffMarker, ResolutionDiffResult, ResolutionGapFact,
    ResolutionGapKind, ResolutionGapTable, ResolutionPendingTombstone, ResolutionScratchCounts,
    ResolutionScratchDelta, ResolutionScratchDeltaReader, ResolutionScratchReader,
    ResolutionScratchWriter, apply_base_delta, resolution_scratch_catalog_hash,
    resolution_scratch_catalog_hash_for_sql, scratch_identifier_target_set,
    scratch_resolution_counts, scratch_semantic_counts, stream_resolution_diff,
    stream_resolution_diff_with_markers,
};
pub use schema::{
    STORE_FORMAT_EPOCH, STORE_SQLITE_SCHEMA_VERSION, StoreSchemaError, create_coordinator_schema,
    create_store_schema,
};
pub use scope::{
    RESOLUTION_SCOPE_JOURNAL_VERSION, RESOLUTION_SCOPE_MAX_CHANGES, ResolutionScopeBatch,
    ResolutionScopeChange, ResolutionScopeChangeKind, ResolutionScopeError, ResolutionScopeState,
    ensure_resolution_scope_feature, resolution_scope_batch, resolution_scope_journal_version,
    resolution_scope_state, validate_resolution_scope_batch,
};
pub use writer::{
    StoreVersionState, StoreWriteRequest, StoreWriteResult, StoreWriter, StoreWriterError,
    StoredFileVersion,
};
