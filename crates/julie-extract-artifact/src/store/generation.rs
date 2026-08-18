use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use super::coordinator::process_status;
use super::layout::{
    initialize_store_database, named_generations, reap_retired_resolution_files, sync_directory,
    sync_file,
};
use super::schema::retire_resolution_store_objects;
use super::maintenance::{
    CapacityProvider, MaintenanceError, MaintenanceExecutor, MaintenancePlan, MaintenanceRun,
};
use super::{
    MaintenanceAction, PartialGenerationOwner, PidStatus, StoreConnectionError,
    StoreConnectionFactory, StoreLayout, StoreLayoutError, StoreSchemaError,
    write_partial_generation_owner,
};

const DEFAULT_COPY_WINDOW: usize = 512;
const MAX_COPY_WINDOW: usize = 2_000;
const DEFAULT_ROLLBACK_SAFETY_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationPolicy {
    pub copy_window: usize,
    pub retained_generation_limit: usize,
    pub rollback_safety_ms: i64,
}

impl Default for GenerationPolicy {
    fn default() -> Self {
        Self {
            copy_window: DEFAULT_COPY_WINDOW,
            retained_generation_limit: 2,
            rollback_safety_ms: DEFAULT_ROLLBACK_SAFETY_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationApplyReport {
    pub source_generation: String,
    pub destination_generation: String,
    pub selected_generation: Option<String>,
    pub copied_file_versions: usize,
    pub copied_rows: usize,
    pub max_observed_copy_window: usize,
    pub copied_base_files: usize,
    pub removed_generations: Vec<String>,
    pub recovered_partial: bool,
    pub repair_disposition: Option<RepairDisposition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairDisposition {
    CheckpointRecovered,
    TornStateRecovered,
    GenerationRebuilt,
}

pub struct GenerationLifecycle {
    executor: MaintenanceExecutor,
    action: MaintenanceAction,
}

impl GenerationLifecycle {
    pub fn acquire(
        factory: StoreConnectionFactory,
        run: MaintenanceRun,
        plan: &MaintenancePlan,
        action: MaintenanceAction,
        capacity: impl CapacityProvider + Send + Sync + 'static,
    ) -> Result<Self, GenerationError> {
        if action == MaintenanceAction::Gc {
            return Err(GenerationError::InvalidAction(action));
        }
        let executor =
            MaintenanceExecutor::acquire_for_action(factory, run, plan, action, capacity)?;
        executor.release_writer_for_generation_build(plan)?;
        Ok(Self { executor, action })
    }

    pub fn promote(
        &mut self,
        plan: &MaintenancePlan,
        policy: &GenerationPolicy,
    ) -> Result<GenerationApplyReport, GenerationError> {
        if !matches!(
            self.action,
            MaintenanceAction::Promote | MaintenanceAction::Repair
        ) {
            return Err(GenerationError::InvalidAction(self.action));
        }
        self.build_and_publish(plan, policy, None)
    }

    pub fn rollback(
        &mut self,
        plan: &MaintenancePlan,
        policy: &GenerationPolicy,
        selected_generation: &str,
    ) -> Result<GenerationApplyReport, GenerationError> {
        if self.action != MaintenanceAction::Rollback {
            return Err(GenerationError::InvalidAction(self.action));
        }
        let selected = StoreLayout::open_named_generation(
            self.executor.factory().layout().root(),
            selected_generation,
        )?;
        if selected.generation_name() == self.executor.factory().layout().generation_name()
            || metadata_value(selected.store_db(), "generation_state")? != "retired"
        {
            return Err(GenerationError::Validation {
                check: "rollback_generation_state",
                detail: selected_generation.to_string(),
            });
        }
        let current = self.executor.factory().layout();
        let family_id = metadata_value(current.store_db(), "family_id")?;
        let binary_version = metadata_value(current.store_db(), "binary_version")?;
        StoreConnectionFactory::new(selected.clone(), family_id, binary_version).open_reader()?;
        self.build_and_publish(plan, policy, Some(selected))
    }

    pub fn repair(
        &mut self,
        plan: &MaintenancePlan,
        policy: &GenerationPolicy,
    ) -> Result<GenerationApplyReport, GenerationError> {
        if self.action != MaintenanceAction::Repair {
            return Err(GenerationError::InvalidAction(self.action));
        }
        let source = self.executor.factory().layout().clone();
        if source.generation_dir().join("OWNER.json").exists() || has_named_successor(&source)? {
            let mut report = self.build_and_publish(plan, policy, None)?;
            report.repair_disposition = Some(RepairDisposition::TornStateRecovered);
            return Ok(report);
        }
        self.executor
            .reacquire_writer_for_generation_publish(plan)?;
        let connection = Connection::open(source.store_db())?;
        connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        MaintenanceExecutor::step_incremental_vacuum(&connection, 256)?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let valid = database_is_valid(&connection)?;
        drop(connection);
        if valid {
            self.executor.finish_generation_action()?;
            return Ok(GenerationApplyReport {
                source_generation: source.generation_name().to_string(),
                destination_generation: source.generation_name().to_string(),
                selected_generation: None,
                copied_file_versions: 0,
                copied_rows: 0,
                max_observed_copy_window: 0,
                copied_base_files: 0,
                removed_generations: Vec::new(),
                recovered_partial: false,
                repair_disposition: Some(RepairDisposition::CheckpointRecovered),
            });
        }
        if !plan.capacity.promotion_fits {
            self.executor.finish_generation_action()?;
            return Err(GenerationError::Maintenance(
                MaintenanceError::CapacityInsufficient,
            ));
        }
        self.executor.release_writer_for_generation_build(plan)?;
        let mut report = self.build_and_publish(plan, policy, None)?;
        report.repair_disposition = Some(RepairDisposition::GenerationRebuilt);
        Ok(report)
    }

    fn build_and_publish(
        &mut self,
        plan: &MaintenancePlan,
        policy: &GenerationPolicy,
        selected: Option<StoreLayout>,
    ) -> Result<GenerationApplyReport, GenerationError> {
        validate_policy(policy)?;
        let source = self.executor.factory().layout().clone();
        retire_source_resolution_objects(&source)?;
        let root = source.root();
        if let Some(report) = self.recover_current_publication(plan, policy, &source)? {
            return Ok(report);
        }
        if let Some(report) = self.recover_named_successor(plan, policy, &source)? {
            return Ok(report);
        }
        // Live free-bytes re-probe before generation staging/create.
        self.executor.ensure_promotion_capacity(plan)?;
        let destination_name = next_generation_name(root)?;
        let partial_name = format!(".{destination_name}.partial");
        let partial = root.join(&partial_name);
        let destination_dir = root.join(&destination_name);
        if destination_dir.exists() {
            return Err(GenerationError::DestinationExists(destination_dir));
        }
        let recovered_partial = recover_or_create_partial(
            &partial,
            self.executor.run(),
            self.executor.fencing_token(),
        )?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("generation_after_partial_owner");
        let partial_store = partial.join("store.db");
        let family_id = metadata_value(source.store_db(), "family_id")?;
        let binary_version = metadata_value(source.store_db(), "binary_version")?;
        if !partial_store.exists() {
            fs::create_dir_all(partial.join("bases"))?;
            initialize_store_database(&partial_store, &family_id, &binary_version)?;
        }

        advance_family_allocator_marks(&source)?;
        self.executor.heartbeat_generation_build()?;
        let mut copy =
            logical_copy_generation(&source, &partial_store, policy.copy_window, &self.executor)?;
        validate_logical_copy(&source, &partial_store)?;
        if let Some(selected) = selected.as_ref() {
            copy.file_versions += merge_selected_immutable_rows(
                selected,
                &partial_store,
                policy.copy_window,
                &self.executor,
            )?;
            apply_forward_rollback(
                &source,
                selected,
                &partial_store,
                self.executor.run().run_id.as_str(),
            )?;
        }
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("generation_after_logical_copy");
        raise_destination_allocators(&source, &partial_store)?;
        let copied_base_files = 0;
        self.executor.heartbeat_generation_build()?;
        validate_destination(&partial_store, &family_id, &partial)?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("generation_after_validation");
        checkpoint_and_sync(&partial_store, &partial)?;
        sync_directory(&partial)?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("generation_before_directory_rename");
        fs::rename(&partial, &destination_dir)?;
        sync_directory(root)?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("generation_after_directory_rename");

        self.executor
            .reacquire_writer_for_generation_publish(plan)?;
        publish_current(root, &source, &destination_name, &destination_dir)?;
        self.executor.finish_generation_action()?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("generation_after_maintenance_finish");
        let owner_path = destination_dir.join("OWNER.json");
        if owner_path.exists() {
            fs::remove_file(owner_path)?;
            sync_directory(&destination_dir)?;
        }
        let removed_generations = cleanup_retired_generations(root, &destination_name, policy)?;
        Ok(GenerationApplyReport {
            source_generation: source.generation_name().to_string(),
            destination_generation: destination_name,
            selected_generation: selected.map(|layout| layout.generation_name().to_string()),
            copied_file_versions: copy.file_versions,
            copied_rows: copy.rows,
            max_observed_copy_window: copy.max_observed_window,
            copied_base_files,
            removed_generations,
            recovered_partial,
            repair_disposition: None,
        })
    }

    fn recover_current_publication(
        &self,
        plan: &MaintenancePlan,
        policy: &GenerationPolicy,
        current: &StoreLayout,
    ) -> Result<Option<GenerationApplyReport>, GenerationError> {
        let owner_path = current.generation_dir().join("OWNER.json");
        if !owner_path.exists() {
            return Ok(None);
        }
        let owner = read_generation_owner(&owner_path)?;
        if owner.run_id == "layout-create" {
            return Ok(None);
        }
        let family_id = metadata_value(current.store_db(), "family_id")?;
        validate_destination(current.store_db(), &family_id, current.generation_dir())?;
        self.executor
            .reacquire_writer_for_generation_publish(plan)?;
        for generation in named_generations(current.root())? {
            if generation != current.generation_name() {
                let layout = StoreLayout::open_named_generation(current.root(), &generation)?;
                if metadata_value(layout.store_db(), "generation_state")? == "serving" {
                    retire_generation(layout.store_db())?;
                    sync_file(layout.store_db())?;
                }
            }
        }
        serve_generation(current.store_db())?;
        sync_file(current.store_db())?;
        self.executor.finish_generation_action()?;
        fs::remove_file(owner_path)?;
        sync_directory(current.generation_dir())?;
        let previous = previous_generation_name(current.root(), current.generation_name())
            .unwrap_or_else(|| current.generation_name().to_string());
        let removed_generations =
            cleanup_retired_generations(current.root(), current.generation_name(), policy)?;
        Ok(Some(GenerationApplyReport {
            source_generation: previous,
            destination_generation: current.generation_name().to_string(),
            selected_generation: None,
            copied_file_versions: count_rows(current.store_db(), "file_versions")?,
            copied_rows: 0,
            max_observed_copy_window: 0,
            copied_base_files: 0,
            removed_generations,
            recovered_partial: true,
            repair_disposition: None,
        }))
    }

    fn recover_named_successor(
        &self,
        plan: &MaintenancePlan,
        policy: &GenerationPolicy,
        source: &StoreLayout,
    ) -> Result<Option<GenerationApplyReport>, GenerationError> {
        let source_number = generation_number(source.generation_name())?;
        let successor_name = named_generations(source.root())?
            .into_iter()
            .filter_map(|name| {
                generation_number(&name)
                    .ok()
                    .filter(|number| *number > source_number)
                    .map(|number| (number, name))
            })
            .min_by_key(|(number, _)| *number)
            .map(|(_, name)| name);
        let Some(successor_name) = successor_name else {
            return Ok(None);
        };
        let successor = StoreLayout::open_named_generation(source.root(), &successor_name)?;
        let owner_path = successor.generation_dir().join("OWNER.json");
        if !owner_path.exists() {
            return Err(GenerationError::Validation {
                check: "unpublished_generation_owner",
                detail: successor_name,
            });
        }
        let family_id = metadata_value(source.store_db(), "family_id")?;
        validate_destination(successor.store_db(), &family_id, successor.generation_dir())?;
        self.executor
            .reacquire_writer_for_generation_publish(plan)?;
        publish_current(
            source.root(),
            source,
            successor.generation_name(),
            successor.generation_dir(),
        )?;
        self.executor.finish_generation_action()?;
        fs::remove_file(owner_path)?;
        sync_directory(successor.generation_dir())?;
        let removed_generations =
            cleanup_retired_generations(source.root(), successor.generation_name(), policy)?;
        Ok(Some(GenerationApplyReport {
            source_generation: source.generation_name().to_string(),
            destination_generation: successor.generation_name().to_string(),
            selected_generation: None,
            copied_file_versions: count_rows(successor.store_db(), "file_versions")?,
            copied_rows: 0,
            max_observed_copy_window: 0,
            copied_base_files: 0,
            removed_generations,
            recovered_partial: true,
            repair_disposition: None,
        }))
    }
}

#[derive(Debug)]
pub enum GenerationError {
    InvalidAction(MaintenanceAction),
    InvalidPolicy { field: &'static str, value: usize },
    DestinationExists(PathBuf),
    InvalidGenerationName(String),
    GenerationOverflow,
    PartialOwned(PathBuf),
    InvalidBasePath(String),
    BaseIdentityMismatch { path: String },
    IdentityConflict { table: String, key: String },
    Validation { check: &'static str, detail: String },
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Layout(StoreLayoutError),
    Maintenance(MaintenanceError),
    Connection(StoreConnectionError),
}

impl GenerationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAction(_) => "generation_invalid_action",
            Self::InvalidPolicy { .. } => "generation_invalid_policy",
            Self::DestinationExists(_) => "generation_destination_exists",
            Self::InvalidGenerationName(_) => "generation_invalid_name",
            Self::GenerationOverflow => "generation_out_of_range",
            Self::PartialOwned(_) => "generation_partial_owned",
            Self::InvalidBasePath(_) => "generation_invalid_base_path",
            Self::BaseIdentityMismatch { .. } => "generation_base_identity_mismatch",
            Self::IdentityConflict { .. } => "generation_identity_conflict",
            Self::Validation { .. } => "generation_validation_failed",
            Self::Io(_) | Self::Sqlite(_) | Self::Layout(_) | Self::Connection(_) => {
                "generation_io_failed"
            }
            Self::Maintenance(error) => error.code(),
        }
    }
}

impl fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.code())
    }
}

impl Error for GenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::Maintenance(error) => Some(error),
            Self::Connection(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for GenerationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for GenerationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreLayoutError> for GenerationError {
    fn from(error: StoreLayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<MaintenanceError> for GenerationError {
    fn from(error: MaintenanceError) -> Self {
        Self::Maintenance(error)
    }
}

impl From<StoreConnectionError> for GenerationError {
    fn from(error: StoreConnectionError) -> Self {
        Self::Connection(error)
    }
}

#[derive(Default)]
struct CopyCounts {
    rows: usize,
    file_versions: usize,
    max_observed_window: usize,
}

struct TableCopyCounts {
    rows: usize,
    max_observed_window: usize,
}

fn validate_policy(policy: &GenerationPolicy) -> Result<(), GenerationError> {
    if policy.copy_window == 0 || policy.copy_window > MAX_COPY_WINDOW {
        return Err(GenerationError::InvalidPolicy {
            field: "copy_window",
            value: policy.copy_window,
        });
    }
    if policy.retained_generation_limit == 0 {
        return Err(GenerationError::InvalidPolicy {
            field: "retained_generation_limit",
            value: policy.retained_generation_limit,
        });
    }
    if policy.rollback_safety_ms < 0 {
        return Err(GenerationError::InvalidPolicy {
            field: "rollback_safety_ms",
            value: policy.rollback_safety_ms.unsigned_abs() as usize,
        });
    }
    Ok(())
}

fn next_generation_name(root: &Path) -> Result<String, GenerationError> {
    let mut maximum = 0_u64;
    for name in named_generations(root)? {
        let number = name
            .strip_prefix("gen-")
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| GenerationError::InvalidGenerationName(name.clone()))?;
        maximum = maximum.max(number);
    }
    let next = maximum
        .checked_add(1)
        .ok_or(GenerationError::GenerationOverflow)?;
    Ok(format!("gen-{next:03}"))
}

fn generation_number(name: &str) -> Result<u64, GenerationError> {
    name.strip_prefix("gen-")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| GenerationError::InvalidGenerationName(name.to_string()))
}

fn previous_generation_name(root: &Path, current: &str) -> Option<String> {
    let current_number = generation_number(current).ok()?;
    named_generations(root)
        .ok()?
        .into_iter()
        .filter_map(|name| {
            generation_number(&name)
                .ok()
                .filter(|number| *number < current_number)
                .map(|number| (number, name))
        })
        .max_by_key(|(number, _)| *number)
        .map(|(_, name)| name)
}

fn has_named_successor(layout: &StoreLayout) -> Result<bool, GenerationError> {
    let current = generation_number(layout.generation_name())?;
    Ok(named_generations(layout.root())?
        .into_iter()
        .filter_map(|name| generation_number(&name).ok())
        .any(|generation| generation > current))
}

fn recover_or_create_partial(
    partial: &Path,
    run: &MaintenanceRun,
    fencing_token: i64,
) -> Result<bool, GenerationError> {
    let recovered = partial.exists();
    if recovered {
        let owner_path = partial.join("OWNER.json");
        let owner: PartialGenerationOwner = serde_json::from_slice(&fs::read(&owner_path)?)
            .map_err(|_| GenerationError::PartialOwned(partial.to_path_buf()))?;
        if owner.run_id != run.run_id
            || owner.owner_id != run.owner_id
            || owner.owner_pid != run.owner_pid
            || owner.fencing_token != fencing_token
        {
            let dead = process_status(owner.owner_pid) == PidStatus::Dead;
            if !dead {
                return Err(GenerationError::PartialOwned(partial.to_path_buf()));
            }
        }
        fs::remove_dir_all(partial)?;
    }
    fs::create_dir(partial)?;
    write_partial_generation_owner(
        partial,
        &PartialGenerationOwner {
            run_id: run.run_id.clone(),
            owner_id: run.owner_id.clone(),
            owner_pid: run.owner_pid,
            fencing_token,
            expires_at: wall_now_ms()?.saturating_add(run.lease_duration_ms),
        },
    )?;
    Ok(recovered)
}

fn read_generation_owner(path: &Path) -> Result<PartialGenerationOwner, GenerationError> {
    serde_json::from_slice(&fs::read(path)?).map_err(|_| GenerationError::Validation {
        check: "generation_owner",
        detail: path.display().to_string(),
    })
}

fn count_rows(path: &Path, table: &str) -> Result<usize, GenerationError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(connection.query_row(
        &format!("SELECT COUNT(*) FROM {}", quote_identifier(table)),
        [],
        |row| row.get::<_, i64>(0),
    )? as usize)
}

fn logical_copy_generation(
    source: &StoreLayout,
    destination_path: &Path,
    window: usize,
    executor: &MaintenanceExecutor,
) -> Result<CopyCounts, GenerationError> {
    let source_connection = Connection::open_with_flags(
        source.store_db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    source_connection.execute_batch("PRAGMA query_only=ON; PRAGMA foreign_keys=ON;")?;
    let mut destination = Connection::open(destination_path)?;
    destination.execute_batch("PRAGMA foreign_keys=OFF;")?;
    copy_store_metadata(
        &source_connection,
        &mut destination,
        executor.source_min_writer_version(),
    )?;
    let mut tables = table_names(&source_connection)?;
    tables.retain(|table| table != "store_meta" && !is_retired_resolution_table(table));
    let mut counts = CopyCounts::default();
    for table in tables {
        let copied = copy_table(
            &source_connection,
            &mut destination,
            &table,
            window,
            false,
            Some(executor),
        )?;
        counts.rows += copied.rows;
        counts.max_observed_window = counts.max_observed_window.max(copied.max_observed_window);
        if table == "file_versions" {
            counts.file_versions = copied.rows;
        }
    }
    destination.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(counts)
}

fn validate_logical_copy(
    source: &StoreLayout,
    destination_path: &Path,
) -> Result<(), GenerationError> {
    let source_connection = Connection::open_with_flags(
        source.store_db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let destination = Connection::open_with_flags(
        destination_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let source_catalog = catalog_rows(&source_connection)?;
    let destination_catalog = catalog_rows(&destination)?;
    if source_catalog != destination_catalog {
        return Err(GenerationError::Validation {
            check: "catalog_fingerprint",
            detail: "source and destination catalogs differ".to_string(),
        });
    }
    for table in table_names(&source_connection)? {
        if table == "store_meta" || is_retired_resolution_table(&table) {
            continue;
        }
        let source_count = table_count(&source_connection, &table)?;
        let destination_count = table_count(&destination, &table)?;
        if source_count != destination_count {
            return Err(GenerationError::Validation {
                check: "row_count",
                detail: format!("{table}:{source_count}:{destination_count}"),
            });
        }
    }
    let mirrored = destination.query_row(
        "SELECT EXISTS(SELECT 1 FROM store_meta WHERE key LIKE 'maintenance_tmp_%')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if mirrored {
        return Err(GenerationError::Validation {
            check: "destination_maintenance_tmp",
            detail: "temporary intent mirrors must not remain on destination".to_string(),
        });
    }
    Ok(())
}

fn is_retired_resolution_table(table: &str) -> bool {
    table.starts_with("resolution_")
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, GenerationError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn retire_source_resolution_objects(source: &StoreLayout) -> Result<(), GenerationError> {
    let connection = Connection::open(source.store_db())?;
    retire_resolution_store_objects(&connection).map_err(|error| match error {
        StoreSchemaError::Sqlite(inner) => GenerationError::Sqlite(inner),
        other => GenerationError::Validation {
            check: "resolution_retirement",
            detail: other.to_string(),
        },
    })?;
    reap_retired_resolution_files(source)?;
    Ok(())
}

fn catalog_rows(
    connection: &Connection,
) -> Result<Vec<(String, String, String, String)>, GenerationError> {
    let mut statement = connection.prepare(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema
         WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'
         ORDER BY type,name",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn table_count(connection: &Connection, table: &str) -> Result<i64, GenerationError> {
    Ok(connection.query_row(
        &format!("SELECT COUNT(*) FROM {}", quote_identifier(table)),
        [],
        |row| row.get(0),
    )?)
}

fn copy_store_metadata(
    source: &Connection,
    destination: &mut Connection,
    source_min_writer_version: &str,
) -> Result<(), GenerationError> {
    let mut statement = source.prepare("SELECT key,value FROM store_meta ORDER BY key")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let transaction = destination.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for (key, value) in rows {
        if key == "generation_state" || key.starts_with("maintenance_tmp_") {
            continue;
        }
        let value = if key == "min_writer_version" {
            source_min_writer_version
        } else {
            value.as_str()
        };
        transaction.execute(
            "INSERT INTO store_meta(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
    }
    // M5: never leave temporary intent mirrors on the destination generation.
    transaction.execute(
        "DELETE FROM store_meta WHERE key LIKE 'maintenance_tmp_%'",
        [],
    )?;
    transaction.execute(
        "UPDATE store_meta SET value='retired' WHERE key='generation_state'",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn table_names(connection: &Connection) -> Result<Vec<String>, GenerationError> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_schema
         WHERE type='table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )?;
    Ok(statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn copy_table(
    source: &Connection,
    destination: &mut Connection,
    table: &str,
    window: usize,
    ignore_conflicts: bool,
    heartbeat: Option<&MaintenanceExecutor>,
) -> Result<TableCopyCounts, GenerationError> {
    let columns = table_columns(source, table)?;
    if columns.is_empty() {
        return Ok(TableCopyCounts {
            rows: 0,
            max_observed_window: 0,
        });
    }
    let column_list = columns
        .iter()
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(",");
    let mut keys = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.primary_key > 0)
        .collect::<Vec<_>>();
    keys.sort_by_key(|(_, column)| column.primary_key);
    let order = if keys.is_empty() {
        column_list.clone()
    } else {
        keys.iter()
            .map(|(_, column)| quote_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(",")
    };
    let table_name = quote_identifier(table);
    let select =
        format!("SELECT {column_list} FROM {table_name} ORDER BY {order} LIMIT ?1 OFFSET ?2");
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let insert_mode = if ignore_conflicts {
        "INSERT OR IGNORE"
    } else {
        "INSERT"
    };
    let insert = format!("{insert_mode} INTO {table_name}({column_list}) VALUES ({placeholders})");
    let lookup = if ignore_conflicts && !keys.is_empty() {
        Some(format!(
            "SELECT {column_list} FROM {table_name} WHERE {}",
            keys.iter()
                .enumerate()
                .map(|(parameter, (_, column))| format!(
                    "{}=?{}",
                    quote_identifier(&column.name),
                    parameter + 1
                ))
                .collect::<Vec<_>>()
                .join(" AND ")
        ))
    } else {
        None
    };
    let mut offset = 0_i64;
    let mut total = 0_usize;
    let mut max_observed_window = 0_usize;
    loop {
        let rows = {
            let mut statement = source.prepare(&select)?;
            let column_count = columns.len();
            statement
                .query_map(params![window as i64, offset], |row| {
                    (0..column_count)
                        .map(|index| value_from_ref(row.get_ref(index)?))
                        .collect::<Result<Vec<_>, _>>()
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if rows.is_empty() {
            break;
        }
        let row_count = rows.len();
        max_observed_window = max_observed_window.max(row_count);
        let transaction = destination.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut statement = transaction.prepare(&insert)?;
            let mut lookup_statement = lookup
                .as_ref()
                .map(|query| transaction.prepare(query))
                .transpose()?;
            for row in rows {
                let changed = statement.execute(rusqlite::params_from_iter(row.iter()))?;
                if changed == 0 && ignore_conflicts {
                    let key_values = keys
                        .iter()
                        .map(|(index, _)| &row[*index])
                        .collect::<Vec<_>>();
                    let existing = lookup_statement
                        .as_mut()
                        .ok_or_else(|| GenerationError::IdentityConflict {
                            table: table.to_string(),
                            key: "missing_primary_key".to_string(),
                        })?
                        .query_row(rusqlite::params_from_iter(key_values), |existing| {
                            (0..columns.len())
                                .map(|index| value_from_ref(existing.get_ref(index)?))
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .optional()?;
                    if existing.as_ref() != Some(&row) {
                        return Err(GenerationError::IdentityConflict {
                            table: table.to_string(),
                            key: keys
                                .iter()
                                .map(|(index, _)| format!("{:?}", row[*index]))
                                .collect::<Vec<_>>()
                                .join(":"),
                        });
                    }
                }
            }
        }
        transaction.commit()?;
        if let Some(executor) = heartbeat {
            executor.heartbeat_generation_build()?;
        }
        total += row_count;
        offset = offset
            .checked_add(row_count as i64)
            .ok_or(GenerationError::GenerationOverflow)?;
    }
    Ok(TableCopyCounts {
        rows: total,
        max_observed_window,
    })
}

#[derive(Debug)]
struct TableColumn {
    name: String,
    primary_key: i64,
}

fn table_columns(
    connection: &Connection,
    table: &str,
) -> Result<Vec<TableColumn>, GenerationError> {
    let mut statement =
        connection.prepare(&format!("PRAGMA table_info({})", quote_identifier(table)))?;
    Ok(statement
        .query_map([], |row| {
            Ok(TableColumn {
                name: row.get(1)?,
                primary_key: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn value_from_ref(value: ValueRef<'_>) -> Result<Value, rusqlite::Error> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Integer(value),
        ValueRef::Real(value) => Value::Real(value),
        ValueRef::Text(value) => Value::Text(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::Blob(value.to_vec()),
    })
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn merge_selected_immutable_rows(
    selected: &StoreLayout,
    destination_path: &Path,
    window: usize,
    executor: &MaintenanceExecutor,
) -> Result<usize, GenerationError> {
    let selected_connection = Connection::open_with_flags(
        selected.store_db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut destination = Connection::open(destination_path)?;
    destination.execute_batch("PRAGMA foreign_keys=OFF;")?;
    let excluded = BTreeSet::from([
        "store_meta",
        "views",
        "manifests",
        "manifest_entries",
        "store_log",
        "request_chunks",
    ]);
    let mut added_versions = 0;
    for table in table_names(&selected_connection)? {
        if excluded.contains(table.as_str()) || is_retired_resolution_table(&table) {
            continue;
        }
        let before = if table == "file_versions" {
            destination.query_row("SELECT COUNT(*) FROM file_versions", [], |row| {
                row.get::<_, i64>(0)
            })? as usize
        } else {
            0
        };
        copy_table(
            &selected_connection,
            &mut destination,
            &table,
            window,
            true,
            Some(executor),
        )?;
        if table == "file_versions" {
            let after = destination.query_row("SELECT COUNT(*) FROM file_versions", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;
            added_versions = after.saturating_sub(before);
        }
    }
    destination.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(added_versions)
}

fn apply_forward_rollback(
    source: &StoreLayout,
    selected: &StoreLayout,
    destination_path: &Path,
    request_id: &str,
) -> Result<(), GenerationError> {
    let selected_connection = Connection::open_with_flags(
        selected.store_db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let views = selected_views(&selected_connection)?;
    let mut allocations = Vec::with_capacity(views.len());
    let mut coord = Connection::open(source.coordinator_db())?;
    let coord_transaction = coord.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for view in &views {
        let manifest_generation =
            allocate_scoped_mark(&coord_transaction, "manifest_generation", &view.view_id)?;
        allocations.push((view.view_id.clone(), manifest_generation));
    }
    coord_transaction.commit()?;

    let mut destination = Connection::open(destination_path)?;
    destination.execute_batch("PRAGMA foreign_keys=ON;")?;
    let transaction = destination.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
    for view in views {
        let (_, manifest_generation) = allocations
            .iter()
            .find(|(view_id, _)| view_id == &view.view_id)
            .ok_or_else(|| GenerationError::Validation {
                check: "rollback_allocator",
                detail: view.view_id.clone(),
            })?;
        let selected_manifest =
            view.current_generation
                .ok_or_else(|| GenerationError::Validation {
                    check: "rollback_manifest",
                    detail: view.view_id.clone(),
                })?;
        let (manifest_hash, created_at) = selected_connection.query_row(
            "SELECT manifest_hash,created_at FROM manifests
             WHERE view_id=?1 AND generation=?2",
            params![view.view_id, selected_manifest],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        transaction.execute(
            "UPDATE views SET current_generation=NULL,resolution_state='unbound',
               resolution_base_id=NULL,resolution_delta_generation=NULL,resolution_exact_at=NULL
             WHERE view_id=?1",
            [&view.view_id],
        )?;
        transaction.execute(
            "DELETE FROM manifest_entries
             WHERE view_id=?1 AND generation IN (
               SELECT generation FROM manifests WHERE view_id=?1 AND manifest_hash=?2
             )",
            params![view.view_id, manifest_hash],
        )?;
        transaction.execute(
            "DELETE FROM manifests WHERE view_id=?1 AND manifest_hash=?2",
            params![view.view_id, manifest_hash],
        )?;
        transaction.execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                view.view_id,
                manifest_generation,
                manifest_hash,
                request_id,
                created_at,
            ],
        )?;
        copy_manifest_entries(
            &selected_connection,
            &transaction,
            &view.view_id,
            selected_manifest,
            *manifest_generation,
        )?;
        let resolution_exact_at = if view.resolution_state == "exact" {
            Some(*manifest_generation)
        } else {
            None
        };
        transaction.execute(
            "UPDATE views SET root=?2,current_generation=?3,resolution_state=?4,
               resolution_base_id=?5,resolution_delta_generation=?6,resolution_exact_at=?7,
               created_at=?8,updated_at=?9
             WHERE view_id=?1",
            params![
                view.view_id,
                view.root,
                manifest_generation,
                view.resolution_state,
                view.resolution_base_id,
                view.resolution_delta_generation,
                resolution_exact_at,
                view.created_at,
                view.updated_at,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

#[derive(Debug)]
struct SelectedView {
    view_id: String,
    root: String,
    current_generation: Option<i64>,
    resolution_state: String,
    resolution_base_id: Option<String>,
    resolution_delta_generation: Option<i64>,
    created_at: String,
    updated_at: String,
}

fn selected_views(connection: &Connection) -> Result<Vec<SelectedView>, GenerationError> {
    let mut statement = connection.prepare(
        "SELECT view_id,root,current_generation,resolution_state,resolution_base_id,
                resolution_delta_generation,created_at,updated_at
         FROM views WHERE current_generation IS NOT NULL ORDER BY view_id",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok(SelectedView {
                view_id: row.get(0)?,
                root: row.get(1)?,
                current_generation: row.get(2)?,
                resolution_state: row.get(3)?,
                resolution_base_id: row.get(4)?,
                resolution_delta_generation: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn allocate_scoped_mark(
    transaction: &rusqlite::Transaction<'_>,
    kind: &str,
    scope: &str,
) -> Result<i64, GenerationError> {
    let current = transaction
        .query_row(
            "SELECT high_water FROM family_allocator_marks
             WHERE allocator_kind=?1 AND scope_id=?2",
            params![kind, scope],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    let next = current
        .checked_add(1)
        .ok_or(GenerationError::GenerationOverflow)?;
    transaction.execute(
        "INSERT INTO family_allocator_marks(allocator_kind,scope_id,high_water,updated_at)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(allocator_kind,scope_id) DO UPDATE SET
           high_water=excluded.high_water,updated_at=MAX(updated_at,excluded.updated_at)",
        params![kind, scope, next, wall_now_ms()?],
    )?;
    Ok(next)
}

fn copy_manifest_entries(
    selected: &Connection,
    destination: &rusqlite::Transaction<'_>,
    view_id: &str,
    selected_generation: i64,
    destination_generation: i64,
) -> Result<(), GenerationError> {
    let mut statement = selected.prepare(
        "SELECT path,language,version_id,status,observed_content_hash,indexed_at,
                error_class,error_json
         FROM manifest_entries WHERE view_id=?1 AND generation=?2 ORDER BY path",
    )?;
    let rows = statement
        .query_map(params![view_id, selected_generation], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for row in rows {
        destination.execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,
              error_class,error_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                view_id,
                destination_generation,
                row.0,
                row.1,
                row.2,
                row.3,
                row.4,
                row.5,
                row.6,
                row.7,
            ],
        )?;
    }
    Ok(())
}


fn advance_family_allocator_marks(layout: &StoreLayout) -> Result<(), GenerationError> {
    let mut scalar = BTreeMap::from([
        (("file_version".to_string(), String::new()), 0_i64),
        (("store_log".to_string(), String::new()), 0_i64),
    ]);
    let mut scoped = BTreeMap::<(String, String), i64>::new();
    for generation in named_generations(layout.root())? {
        let candidate = StoreLayout::open_named_generation(layout.root(), &generation)?;
        let connection = Connection::open_with_flags(
            candidate.store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        scalar.insert(
            ("file_version".to_string(), String::new()),
            scalar[&("file_version".to_string(), String::new())].max(max_value(
                &connection,
                "SELECT COALESCE(MAX(version_id),0) FROM file_versions",
            )?),
        );
        scalar.insert(
            ("store_log".to_string(), String::new()),
            scalar[&("store_log".to_string(), String::new())].max(max_value(
                &connection,
                "SELECT COALESCE(MAX(sequence),0) FROM store_log",
            )?),
        );
        merge_scoped_maxima(
            &connection,
            "SELECT view_id,MAX(generation) FROM manifests GROUP BY view_id",
            "manifest_generation",
            &mut scoped,
        )?;
    }
    let mut coord = Connection::open(layout.coordinator_db())?;
    let receipt_max = coord.query_row(
        "SELECT COALESCE(MAX(terminal_log_sequence),0) FROM request_receipts",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if let Some(value) = scalar.get_mut(&("store_log".to_string(), String::new())) {
        *value = (*value).max(receipt_max);
    }
    let transaction = coord.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for ((kind, scope), high_water) in scalar.into_iter().chain(scoped) {
        transaction.execute(
            "INSERT INTO family_allocator_marks(allocator_kind,scope_id,high_water,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(allocator_kind,scope_id) DO UPDATE SET
               high_water=MAX(high_water,excluded.high_water),
               updated_at=MAX(updated_at,excluded.updated_at)",
            params![kind, scope, high_water, wall_now_ms()?],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn merge_scoped_maxima(
    connection: &Connection,
    query: &str,
    kind: &str,
    maxima: &mut BTreeMap<(String, String), i64>,
) -> Result<(), GenerationError> {
    let mut statement = connection.prepare(query)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (scope, value) in rows {
        maxima
            .entry((kind.to_string(), scope))
            .and_modify(|current| *current = (*current).max(value))
            .or_insert(value);
    }
    Ok(())
}

fn max_value(connection: &Connection, query: &str) -> Result<i64, GenerationError> {
    Ok(connection.query_row(query, [], |row| row.get(0))?)
}

fn raise_destination_allocators(
    layout: &StoreLayout,
    destination_path: &Path,
) -> Result<(), GenerationError> {
    let coord = Connection::open_with_flags(
        layout.coordinator_db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let marks = {
        let mut statement = coord.prepare(
            "SELECT allocator_kind,scope_id,high_water FROM family_allocator_marks
             ORDER BY allocator_kind,scope_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut destination = Connection::open(destination_path)?;
    let transaction = destination.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for (kind, _scope, high_water) in marks {
        let table = match kind.as_str() {
            "file_version" => Some("file_versions"),
            "store_log" => Some("store_log"),
            _ => None,
        };
        if let Some(table) = table {
            let changed = transaction.execute(
                "UPDATE sqlite_sequence SET seq=MAX(seq,?2) WHERE name=?1",
                params![table, high_water],
            )?;
            if changed == 0 {
                transaction.execute(
                    "INSERT INTO sqlite_sequence(name,seq) VALUES (?1,?2)",
                    params![table, high_water],
                )?;
            }
        }
    }
    transaction.commit()?;
    Ok(())
}

fn validate_destination(
    destination_path: &Path,
    family_id: &str,
    partial: &Path,
) -> Result<(), GenerationError> {
    let connection = Connection::open(destination_path)?;
    if metadata_value_from_connection(&connection, "family_id")? != family_id {
        return Err(GenerationError::Validation {
            check: "family_id",
            detail: "destination family changed".to_string(),
        });
    }
    for check in ["quick_check", "integrity_check"] {
        let detail = connection.query_row(&format!("PRAGMA {check}"), [], |row| {
            row.get::<_, String>(0)
        })?;
        if detail != "ok" {
            return Err(GenerationError::Validation { check, detail });
        }
    }
    let foreign_key_error = connection
        .query_row("PRAGMA foreign_key_check", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    if let Some(detail) = foreign_key_error {
        return Err(GenerationError::Validation {
            check: "foreign_key_check",
            detail,
        });
    }
    let _ = partial;
    Ok(())
}

fn database_is_valid(connection: &Connection) -> Result<bool, GenerationError> {
    for check in ["quick_check", "integrity_check"] {
        let detail = connection.query_row(&format!("PRAGMA {check}"), [], |row| {
            row.get::<_, String>(0)
        })?;
        if detail != "ok" {
            return Ok(false);
        }
    }
    Ok(connection
        .query_row("PRAGMA foreign_key_check", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
        .is_none())
}

fn checkpoint_and_sync(store_path: &Path, partial: &Path) -> Result<(), GenerationError> {
    let connection = Connection::open(store_path)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);
    sync_file(store_path)?;
    sync_directory(&partial.join("bases"))?;
    sync_directory(partial)?;
    Ok(())
}

fn publish_current(
    root: &Path,
    source: &StoreLayout,
    destination_name: &str,
    destination_dir: &Path,
) -> Result<(), GenerationError> {
    let destination = StoreLayout::open_named_generation(root, destination_name)?;
    retire_generation(source.store_db())?;
    sync_file(source.store_db())?;
    #[cfg(feature = "test-store-crash")]
    super::test_hooks::crash_if("generation_after_source_retired");
    let partial_current = root.join("CURRENT.partial");
    let mut current = File::create(&partial_current)?;
    current.write_all(format!("{destination_name}\n").as_bytes())?;
    current.sync_all()?;
    drop(current);
    fs::rename(&partial_current, root.join("CURRENT"))?;
    sync_directory(root)?;
    #[cfg(feature = "test-store-crash")]
    super::test_hooks::crash_if("generation_after_current_publish");
    serve_generation(destination.store_db())?;
    sync_file(destination.store_db())?;
    sync_directory(destination_dir)?;
    #[cfg(feature = "test-store-crash")]
    super::test_hooks::crash_if("generation_after_destination_serving");
    Ok(())
}

fn retire_generation(path: &Path) -> Result<(), GenerationError> {
    let mut connection = Connection::open(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "UPDATE store_meta SET value='retired' WHERE key='generation_state'",
        [],
    )?;
    transaction.execute(
        "INSERT INTO store_meta(key,value) VALUES ('generation_retired_at_ms',?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [wall_now_ms()?.to_string()],
    )?;
    transaction.commit()?;
    Ok(())
}

fn serve_generation(path: &Path) -> Result<(), GenerationError> {
    let mut connection = Connection::open(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "UPDATE store_meta SET value='serving' WHERE key='generation_state'",
        [],
    )?;
    transaction.execute(
        "DELETE FROM store_meta WHERE key='generation_retired_at_ms'",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn cleanup_retired_generations(
    root: &Path,
    current: &str,
    policy: &GenerationPolicy,
) -> Result<Vec<String>, GenerationError> {
    let mut retired = named_generations(root)?
        .into_iter()
        .filter(|name| name != current)
        .map(|name| generation_number(&name).map(|number| (number, name)))
        .collect::<Result<Vec<_>, _>>()?;
    retired.sort_by_key(|(number, _)| *number);
    if retired.len() <= policy.retained_generation_limit {
        return Ok(Vec::new());
    }
    let remove_count = retired.len() - policy.retained_generation_limit;
    let mut removed = Vec::new();
    for (_, name) in retired.drain(..remove_count) {
        let layout = StoreLayout::open_named_generation(root, &name)?;
        let connection = Connection::open(layout.store_db())?;
        let live_pins = if table_exists(&connection, "resolution_pins")? {
            connection.query_row(
                "SELECT COUNT(*) FROM resolution_pins WHERE expires_at>strftime('%Y-%m-%dT%H:%M:%fZ','now')",
                [],
                |row| row.get::<_, i64>(0),
            )?
        } else {
            0
        };
        let retired_at = connection
            .query_row(
                "SELECT value FROM store_meta WHERE key='generation_retired_at_ms'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok());
        let safety_elapsed = retired_at.is_some_and(|retired_at| {
            wall_now_ms()
                .is_ok_and(|now| now >= retired_at.saturating_add(policy.rollback_safety_ms))
        });
        if live_pins == 0 && safety_elapsed {
            drop(connection);
            fs::remove_dir_all(layout.generation_dir())?;
            removed.push(name);
        }
    }
    if !removed.is_empty() {
        sync_directory(root)?;
    }
    Ok(removed)
}

fn metadata_value(path: &Path, key: &str) -> Result<String, GenerationError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    metadata_value_from_connection(&connection, key)
}

fn metadata_value_from_connection(
    connection: &Connection,
    key: &str,
) -> Result<String, GenerationError> {
    Ok(
        connection.query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
            row.get(0)
        })?,
    )
}

fn wall_now_ms() -> Result<i64, GenerationError> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("system clock: {error}"))
        })?;
    i64::try_from(duration.as_millis()).map_err(|_| GenerationError::GenerationOverflow)
}
