pub mod args;
pub(crate) mod common;
mod completion;
mod delete;
mod executor;
mod export;
mod from_artifact;
pub mod import;
mod maintenance;
pub mod maintenance_report;
pub(crate) mod reader;
pub mod report;
#[cfg(feature = "test-store-contract")]
pub mod test_support;
mod update;

use import::StoreExecutionOutcome;

pub fn dispatch(args: args::StoreArgs) -> StoreExecutionOutcome {
    let checkpoint_root = completion::write_command_root(&args.command).map(ToOwned::to_owned);
    let outcome = match args.command {
        args::StoreCommand::Import(args) => import::run(args),
        args::StoreCommand::Update(args) => update::run(args),
        args::StoreCommand::Delete(args) => delete::run(args),
        args::StoreCommand::Export(args) => export::run(args),
        args::StoreCommand::Maintain(args) => maintenance::run(args),
        args::StoreCommand::Reader(args) => reader::run(args),
    };
    // Command-owned connections and transactions have dropped, including the
    // coordinator's final lease writes. Cleanup never changes the report.
    if let Some(root) = checkpoint_root {
        completion::checkpoint(&root, outcome.exit_code() == 0);
    }
    outcome
}

pub use args::{
    DEFAULT_REQUEST_TIMEOUT_SECONDS, MAX_REQUEST_TIMEOUT_SECONDS, MAX_STORE_IDENTIFIER_BYTES,
    MAX_STORE_PATH_BYTES, StoreArgs, StoreCommand, StoreDeleteArgs, StoreExportArgs,
    StoreImportArgs, StoreLevelArg, StoreMaintainArgs, StoreMaintenanceCommand,
    StoreMaintenanceCursorAdvanceArgs, StoreMaintenanceCursorArgs, StoreMaintenanceCursorCommand,
    StoreMaintenanceCursorReleaseArgs, StoreMaintenanceInspectArgs, StoreMaintenanceMutationArgs,
    StoreRequestControls, StoreScanControls, StoreUpdateArgs,
};
pub use maintenance_report::{
    STORE_MAINTENANCE_REPORT_SCHEMA_VERSION, StoreMaintenanceAction,
    StoreMaintenanceCapacityReport, StoreMaintenanceCommandOutcome, StoreMaintenanceCounts,
    StoreMaintenanceDisposition, StoreMaintenanceErrorReport, StoreMaintenanceFailureClass,
    StoreMaintenanceFingerprints, StoreMaintenanceMode, StoreMaintenanceReport,
    StoreMaintenanceRetentionReport,
};
pub use report::{
    STORE_EXIT_INCOMPATIBLE, STORE_EXIT_OPERATIONAL_FAILURE, STORE_EXIT_SUCCESS, STORE_EXIT_USAGE,
    STORE_REPORT_SCHEMA_VERSION, StoreCommandOutcome, StoreCoordinatorDisposition,
    StoreErrorReport, StoreExportDisposition, StoreExportReport, StoreFailureClass,
    StoreLevelCompletion, StoreManifestDisposition, StoreManifestReport, StoreOperation,
    StoreOutputFormat, StoreOutputPlan, StoreOutputStream, StoreReport, StoreRequestReport,
    StoreRequestState, StoreRequestedLevel, StoreRowCounts,
};
