mod connection;
mod coordinator;
mod layout;
mod log;
mod manifest;
mod model;
mod pragmas;
mod resolution;
mod resolution_diff;
mod rows;
mod schema;
#[cfg(feature = "test-store-crash")]
#[doc(hidden)]
pub mod test_hooks;
mod writer;

pub use connection::{StoreConnectionError, StoreConnectionFactory};
pub use coordinator::{
    CoordinatorError, CoordinatorExecutor, CoordinatorPolicy, CoordinatorRequest, DrainReport,
    EnqueueResult, ExecutionContext, ExecutionQuantum, LeaseDisposition, LeaseHolder, LeaseRecord,
    PidLiveness, PidStatus, ReconcileOutcome, RequestKind, RequestState, StoreCoordinator,
    UnixMillisClock, compare_versions,
};
pub use layout::{StoreLayout, StoreLayoutError};
pub use log::{StoreLog, StoreLogEntry, StoreLogError, StoreLogRecord};
pub use manifest::{
    BuiltManifest, MANIFEST_HASH_ALGORITHM, MANIFEST_PUBLISH_MAX_RETRIES, ManifestBuilder,
    ManifestEntry, ManifestEntryStatus, ManifestPublishDisposition, ManifestPublishResult,
    ManifestStore, ManifestStoreError, ViewEnsureDisposition,
};
pub use model::{
    ResolutionBaseRecord, ResolutionBaseState, ResolutionDeltaRecord,
    ResolutionIdentifierDeltaRecord, ResolutionPendingDeltaRecord, ResolutionPendingOperation,
    ResolutionPinOwnerKind, ResolutionPinRecord, StoreFileVersion, StoreLevel,
    StoreProjectionError, StoreReferenceSite, StoreRowCounts, ViewResolutionState,
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
    resolution_base_catalog_hash_for_sql,
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
pub use writer::{
    StoreVersionState, StoreWriteRequest, StoreWriteResult, StoreWriter, StoreWriterError,
    StoredFileVersion,
};
