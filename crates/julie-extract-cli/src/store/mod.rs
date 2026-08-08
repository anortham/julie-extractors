pub mod args;
mod delete;
mod executor;
pub mod import;
pub mod report;
pub mod resolution_session;
#[cfg(feature = "test-store-contract")]
pub mod test_support;
mod update;

use import::StoreExecutionOutcome;

pub fn dispatch(args: args::StoreArgs) -> StoreExecutionOutcome {
    match args.command {
        args::StoreCommand::Import(args) => import::run(args),
        args::StoreCommand::Update(args) => update::run(args),
        args::StoreCommand::Delete(args) => delete::run(args),
    }
}

pub use args::{
    DEFAULT_REQUEST_TIMEOUT_SECONDS, MAX_REQUEST_TIMEOUT_SECONDS, MAX_STORE_IDENTIFIER_BYTES,
    MAX_STORE_PATH_BYTES, StoreArgs, StoreCommand, StoreDeleteArgs, StoreImportArgs, StoreLevelArg,
    StoreRequestControls, StoreScanControls, StoreUpdateArgs,
};
pub use report::{
    STORE_EXIT_INCOMPATIBLE, STORE_EXIT_OPERATIONAL_FAILURE, STORE_EXIT_SUCCESS, STORE_EXIT_USAGE,
    STORE_REPORT_SCHEMA_VERSION, StoreCommandOutcome, StoreCoordinatorDisposition,
    StoreErrorReport, StoreFailureClass, StoreLevelCompletion, StoreManifestDisposition,
    StoreManifestReport, StoreOperation, StoreOutputFormat, StoreOutputPlan, StoreOutputStream,
    StoreReport, StoreRequestReport, StoreRequestState, StoreRequestedLevel, StoreResolutionReport,
    StoreResolutionState, StoreRowCounts,
};
