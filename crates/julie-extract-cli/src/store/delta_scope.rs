use std::collections::BTreeSet;

use julie_extract_artifact::store::{
    ResolutionScopeChangeKind, ResolutionScopeError, ResolutionScopeState, resolution_scope_batch,
    resolution_scope_state, validate_resolution_scope_batch,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::resolution::DELTA_SCOPE_CROSSOVER;
#[cfg(not(test))]
use crate::resolution::{import_binding, import_module_candidates};
use crate::resolution_session::{
    ResolutionPhase, ResolutionWorklistScope, ResolutionWorklists, SemanticVersionId,
};

const SCOPE_QUERY_CHUNK: usize = 300;

#[derive(Debug, Clone, Copy)]
pub(crate) struct StoreDeltaScopeRequest<'a> {
    pub(crate) view_id: &'a str,
    pub(crate) manifest_generation: i64,
    pub(crate) manifest_hash: &'a str,
    pub(crate) resolver_output_epoch: i64,
    pub(crate) incremental_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreDeltaScopeFullReason {
    EnvironmentDisabled,
    ScopeStateMissing,
    CurrentManifestMismatch,
    ResolverEpochMismatch,
    JournalBatchMissing,
    JournalChainBroken,
    JournalCountMismatch,
    JournalHashMismatch,
    JournalInvalid,
    JournalPredecessorMismatch,
    JournalScopeUnusable,
    JournalEpochMismatch,
    Crossover,
}

impl StoreDeltaScopeFullReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::EnvironmentDisabled => "incremental_resolution_disabled",
            Self::ScopeStateMissing => "resolution_scope_state_missing",
            Self::CurrentManifestMismatch => "resolution_scope_current_manifest_mismatch",
            Self::ResolverEpochMismatch => "resolution_scope_resolver_epoch_mismatch",
            Self::JournalBatchMissing => "resolution_scope_journal_batch_missing",
            Self::JournalChainBroken => "resolution_scope_journal_chain_broken",
            Self::JournalCountMismatch => "resolution_scope_journal_count_mismatch",
            Self::JournalHashMismatch => "resolution_scope_journal_hash_mismatch",
            Self::JournalInvalid => "resolution_scope_journal_invalid",
            Self::JournalPredecessorMismatch => "resolution_scope_journal_predecessor_mismatch",
            Self::JournalScopeUnusable => "resolution_scope_journal_unusable",
            Self::JournalEpochMismatch => "resolution_scope_journal_epoch_mismatch",
            Self::Crossover => "resolution_scope_crossover",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StoreDeltaScopeDecision {
    Scoped(ResolutionWorklists),
    Full {
        worklists: ResolutionWorklists,
        reason: StoreDeltaScopeFullReason,
    },
}

impl StoreDeltaScopeDecision {
    pub(crate) fn worklists(&self) -> &ResolutionWorklists {
        match self {
            Self::Scoped(worklists) | Self::Full { worklists, .. } => worklists,
        }
    }
}

pub(crate) fn build_store_delta_scope(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
) -> Result<StoreDeltaScopeDecision, ResolutionScopeError> {
    if !request.incremental_enabled {
        return Ok(full(StoreDeltaScopeFullReason::EnvironmentDisabled));
    }
    let state = match resolution_scope_state(connection, request.view_id) {
        Ok(Some(state)) => state,
        Ok(None) => return Ok(full(StoreDeltaScopeFullReason::ScopeStateMissing)),
        Err(ResolutionScopeError::Sqlite(error)) => {
            return Err(ResolutionScopeError::Sqlite(error));
        }
        Err(_) => return Ok(full(StoreDeltaScopeFullReason::JournalInvalid)),
    };
    if state.current_manifest_generation != request.manifest_generation
        || state.current_manifest_hash != request.manifest_hash
    {
        return Ok(full(StoreDeltaScopeFullReason::CurrentManifestMismatch));
    }
    if state.resolver_output_epoch != request.resolver_output_epoch {
        return Ok(full(StoreDeltaScopeFullReason::ResolverEpochMismatch));
    }

    let changes = match validated_scope_changes(connection, request, &state)? {
        Ok(changes) => changes,
        Err(reason) => return Ok(full(reason)),
    };
    let mut touched_names = BTreeSet::new();
    let mut changed_paths = BTreeSet::new();
    let mut structural_paths = BTreeSet::new();
    let mut changed_versions = BTreeSet::new();
    let mut affected_version_ids = BTreeSet::new();
    for change in changes {
        changed_paths.insert(change.path.clone());
        let Ok(names) = serde_json::from_str::<Vec<String>>(&change.touched_names_json) else {
            return Ok(full(StoreDeltaScopeFullReason::JournalInvalid));
        };
        touched_names.extend(names);
        if let Some(version_id) = change.old_version_id {
            affected_version_ids.insert(version_id);
        }
        if let Some(version_id) = change.new_version_id {
            affected_version_ids.insert(version_id);
            changed_versions.insert(version_id);
        }
        if matches!(
            change.change_kind,
            ResolutionScopeChangeKind::PathAdded | ResolutionScopeChangeKind::PathDeleted
        ) {
            structural_paths.insert(change.path);
        }
    }

    let mut recheck_names = touched_names.clone();
    recheck_names.extend(import_alias_names(connection, request, &touched_names)?);
    recheck_names.extend(receiver_names(connection, request, &touched_names)?);

    let visible_versions = current_manifest_versions(connection, request)?;
    changed_versions.retain(|version_id| visible_versions.contains(version_id));
    let module_repoints = module_repoint_scope(connection, request, &structural_paths)?;
    let mut recheck_versions = changed_versions.clone();
    recheck_versions.extend(module_repoints.versions);
    let mut logical_recheck_paths = changed_paths.clone();
    logical_recheck_paths.extend(module_repoints.paths);
    let affected_languages = affected_languages(connection, &affected_version_ids)?;
    if name_expansion_requires_language(&recheck_names, &affected_languages) {
        return Ok(full(StoreDeltaScopeFullReason::JournalInvalid));
    }
    let mut selected_versions = recheck_versions.clone();
    selected_versions.extend(versions_matching_names(
        connection,
        request,
        &recheck_names,
        &affected_languages,
    )?);
    if scope_crosses_over(
        connection,
        request,
        logical_recheck_paths.len(),
        &recheck_names,
        &selected_versions,
    )? {
        return Ok(full(StoreDeltaScopeFullReason::Crossover));
    }

    let recheck_versions = semantic_versions(recheck_versions);
    let selected_versions = semantic_versions(selected_versions);
    let changed_versions = semantic_versions(changed_versions);
    Ok(StoreDeltaScopeDecision::Scoped(ResolutionWorklists {
        scope: ResolutionWorklistScope::Versions(selected_versions.clone()),
        effective_full: false,
        recheck_names: recheck_names.into_iter().collect(),
        recheck_versions,
        selected_versions,
        changed_versions,
        phase: ResolutionPhase::ResolvedPending,
        repair_identifiers: Vec::new(),
    }))
}

fn validated_scope_changes(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
    state: &ResolutionScopeState,
) -> Result<
    Result<Vec<julie_extract_artifact::store::ResolutionScopeChange>, StoreDeltaScopeFullReason>,
    ResolutionScopeError,
> {
    let mut transition_id = state.journal_through_transition_id;
    let mut expected_generation = request.manifest_generation;
    let mut expected_hash = request.manifest_hash.to_string();
    let mut changes = Vec::new();
    loop {
        let batch = match resolution_scope_batch(connection, transition_id) {
            Ok(Some(batch)) => batch,
            Ok(None) => {
                return Ok(Err(StoreDeltaScopeFullReason::JournalBatchMissing));
            }
            Err(ResolutionScopeError::Sqlite(error)) => {
                return Err(ResolutionScopeError::Sqlite(error));
            }
            Err(_) => return Ok(Err(StoreDeltaScopeFullReason::JournalInvalid)),
        };
        if batch.view_id != request.view_id
            || batch.to_manifest_generation != expected_generation
            || batch.to_manifest_hash != expected_hash
        {
            return Ok(Err(StoreDeltaScopeFullReason::JournalChainBroken));
        }
        if !batch.scope_usable {
            return Ok(Err(StoreDeltaScopeFullReason::JournalScopeUnusable));
        }
        if batch.resolver_output_epoch != Some(state.resolver_output_epoch) {
            return Ok(Err(StoreDeltaScopeFullReason::JournalEpochMismatch));
        }
        if batch.predecessor_manifest_generation != Some(state.predecessor_manifest_generation)
            || batch.predecessor_manifest_hash.as_deref()
                != Some(state.predecessor_manifest_hash.as_str())
            || batch.base_id.as_deref() != Some(state.base_id.as_str())
            || batch.delta_generation != Some(state.delta_generation)
        {
            return Ok(Err(StoreDeltaScopeFullReason::JournalPredecessorMismatch));
        }
        match validate_resolution_scope_batch(connection, transition_id) {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(Err(StoreDeltaScopeFullReason::JournalBatchMissing)),
            Err(ResolutionScopeError::InvalidBatch { detail, .. }) => {
                return Ok(Err(invalid_batch_reason(&detail)));
            }
            Err(ResolutionScopeError::Sqlite(error)) => {
                return Err(ResolutionScopeError::Sqlite(error));
            }
            Err(_) => return Ok(Err(StoreDeltaScopeFullReason::JournalInvalid)),
        }
        changes.extend(batch.changes);
        if batch.from_manifest_generation == Some(state.predecessor_manifest_generation)
            && batch.from_manifest_hash.as_deref() == Some(state.predecessor_manifest_hash.as_str())
        {
            break;
        }
        let (Some(previous), Some(from_generation), Some(from_hash)) = (
            batch.previous_transition_id,
            batch.from_manifest_generation,
            batch.from_manifest_hash,
        ) else {
            return Ok(Err(StoreDeltaScopeFullReason::JournalChainBroken));
        };
        transition_id = previous;
        expected_generation = from_generation;
        expected_hash = from_hash;
    }
    Ok(Ok(changes))
}

fn invalid_batch_reason(detail: &str) -> StoreDeltaScopeFullReason {
    if detail.contains("change count") {
        StoreDeltaScopeFullReason::JournalCountMismatch
    } else if detail.contains("change hash") {
        StoreDeltaScopeFullReason::JournalHashMismatch
    } else if detail.contains("previous transition")
        || detail.contains("source manifest")
        || detail.contains("target manifest")
    {
        StoreDeltaScopeFullReason::JournalChainBroken
    } else if detail.contains("predecessor tuple") {
        StoreDeltaScopeFullReason::JournalPredecessorMismatch
    } else {
        StoreDeltaScopeFullReason::JournalInvalid
    }
}

fn current_manifest_versions(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
) -> Result<BTreeSet<i64>, ResolutionScopeError> {
    let mut statement = connection.prepare(
        "SELECT version_id FROM manifest_entries
         WHERE view_id=?1 AND generation=?2
           AND status IN ('indexed','failed_preserved')
         ORDER BY version_id",
    )?;
    Ok(statement
        .query_map(
            params![request.view_id, request.manifest_generation],
            |row| row.get(0),
        )?
        .collect::<Result<_, _>>()?)
}

fn import_alias_names(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
    touched_names: &BTreeSet<String>,
) -> Result<BTreeSet<String>, ResolutionScopeError> {
    let mut statement = connection.prepare(
        "SELECT symbol.name,symbol.metadata_json
         FROM manifest_entries AS entry
         JOIN symbols AS symbol ON symbol.version_id=entry.version_id
         WHERE entry.view_id=?1 AND entry.generation=?2
           AND entry.status IN ('indexed','failed_preserved') AND symbol.kind='import'
         ORDER BY symbol.version_id,symbol.symbol_id COLLATE BINARY",
    )?;
    let rows = statement.query_map(
        params![request.view_id, request.manifest_generation],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;
    let mut linked = BTreeSet::new();
    for row in rows {
        let (name, metadata) = row?;
        let (local_name, imported_name, _, _, _, _) = import_binding(&name, metadata.as_deref());
        if touched_names.contains(&local_name)
            || imported_name
                .as_ref()
                .is_some_and(|name| touched_names.contains(name))
        {
            linked.insert(local_name);
            if let Some(name) = imported_name {
                linked.insert(name);
            }
        }
    }
    Ok(linked)
}

fn receiver_names(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
    touched_names: &BTreeSet<String>,
) -> Result<BTreeSet<String>, ResolutionScopeError> {
    let mut receivers = BTreeSet::new();
    for chunk in string_chunks(touched_names) {
        let sql = format!(
            "SELECT DISTINCT symbol.name
             FROM manifest_entries AS entry
             JOIN symbols AS symbol ON symbol.version_id=entry.version_id
             JOIN type_facts AS fact
               ON fact.version_id=symbol.version_id AND fact.symbol_id=symbol.symbol_id
             WHERE entry.view_id=? AND entry.generation=?
               AND entry.status IN ('indexed','failed_preserved')
               AND fact.resolved_type IN ({})
             ORDER BY symbol.name COLLATE BINARY",
            placeholders(chunk.len())
        );
        let mut bind = vec![
            rusqlite::types::Value::Text(request.view_id.to_string()),
            rusqlite::types::Value::Integer(request.manifest_generation),
        ];
        bind.extend(chunk.into_iter().map(rusqlite::types::Value::Text));
        let mut statement = connection.prepare(&sql)?;
        for row in statement.query_map(rusqlite::params_from_iter(bind), |row| row.get(0))? {
            receivers.insert(row?);
        }
    }
    Ok(receivers)
}

struct ModuleRepointScope {
    versions: BTreeSet<i64>,
    paths: BTreeSet<String>,
}

fn module_repoint_scope(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
    structural_paths: &BTreeSet<String>,
) -> Result<ModuleRepointScope, ResolutionScopeError> {
    if structural_paths.is_empty() {
        return Ok(ModuleRepointScope {
            versions: BTreeSet::new(),
            paths: BTreeSet::new(),
        });
    }
    let mut statement = connection.prepare(
        "SELECT symbol.version_id,symbol.path,symbol.language,symbol.name,symbol.metadata_json
         FROM manifest_entries AS entry
         JOIN symbols AS symbol ON symbol.version_id=entry.version_id
         WHERE entry.view_id=?1 AND entry.generation=?2
           AND entry.status IN ('indexed','failed_preserved') AND symbol.kind='import'
         ORDER BY symbol.version_id,symbol.symbol_id COLLATE BINARY",
    )?;
    let rows = statement.query_map(
        params![request.view_id, request.manifest_generation],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    let mut versions = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for row in rows {
        let (version_id, path, language, name, metadata) = row?;
        let (_, _, source, _, _, _) = import_binding(&name, metadata.as_deref());
        if import_module_candidates(&path, source.as_deref(), &language)
            .iter()
            .any(|candidate| structural_paths.contains(candidate))
        {
            versions.insert(version_id);
            paths.insert(path);
        }
    }
    Ok(ModuleRepointScope { versions, paths })
}

fn affected_languages(
    connection: &Connection,
    version_ids: &BTreeSet<i64>,
) -> Result<BTreeSet<String>, ResolutionScopeError> {
    let mut language_facts = connection.prepare(
        "SELECT language FROM symbols
         WHERE version_id=?1 AND language <> ''
         UNION
         SELECT language FROM reference_sites
         WHERE version_id=?1 AND language <> ''
         ORDER BY language COLLATE BINARY",
    )?;
    let mut file_language =
        connection.prepare("SELECT language FROM file_versions WHERE version_id=?1")?;
    let mut languages = BTreeSet::new();
    for version_id in version_ids {
        let mut recovered = false;
        for row in language_facts.query_map([version_id], |row| row.get::<_, String>(0))? {
            let language = row?;
            if !language.is_empty() {
                languages.insert(language);
                recovered = true;
            }
        }
        if !recovered
            && let Some(language) = file_language
                .query_row([version_id], |row| row.get::<_, String>(0))
                .optional()?
            && !language.is_empty()
        {
            languages.insert(language);
        }
    }
    Ok(languages)
}

fn name_expansion_requires_language(
    recheck_names: &BTreeSet<String>,
    affected_languages: &BTreeSet<String>,
) -> bool {
    !recheck_names.is_empty() && affected_languages.is_empty()
}

#[cfg(test)]
#[test]
fn alias_only_name_expansion_without_recoverable_language_fails_closed() {
    let names = BTreeSet::from(["Alias".to_string()]);
    let languages = BTreeSet::new();

    assert!(name_expansion_requires_language(&names, &languages));
}

fn versions_matching_names(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
    names: &BTreeSet<String>,
    affected_languages: &BTreeSet<String>,
) -> Result<BTreeSet<i64>, ResolutionScopeError> {
    let mut versions = BTreeSet::new();
    for chunk in string_chunks(names) {
        let name_placeholders = placeholders(chunk.len());
        let language_placeholders = placeholders(affected_languages.len());
        let pending_sql = format!(
            "SELECT DISTINCT pending.version_id
             FROM manifest_entries AS entry
             JOIN pending_relationships AS pending ON pending.version_id=entry.version_id
             LEFT JOIN reference_sites AS edge
               ON edge.version_id=pending.version_id
              AND edge.reference_site_id=pending.reference_site_id
             JOIN file_versions AS version ON version.version_id=pending.version_id
             WHERE entry.view_id=? AND entry.generation=?
               AND entry.status IN ('indexed','failed_preserved')
               AND (pending.target_terminal_name IN ({name_placeholders})
                    OR pending.target_receiver IN ({name_placeholders}))
               AND (edge.language IN ({language_placeholders})
                    OR version.language IN ({language_placeholders}))"
        );
        let identifier_sql = format!(
            "SELECT DISTINCT identifier.version_id
             FROM manifest_entries AS entry
             JOIN identifiers AS identifier ON identifier.version_id=entry.version_id
             WHERE entry.view_id=? AND entry.generation=?
               AND entry.status IN ('indexed','failed_preserved')
               AND (identifier.name IN ({name_placeholders})
                    OR json_extract(identifier.metadata_json,'$.receiver') IN ({name_placeholders}))
               AND identifier.language IN ({language_placeholders})"
        );
        for (sql, pending_language) in [(pending_sql, true), (identifier_sql, false)] {
            let mut bind = vec![
                rusqlite::types::Value::Text(request.view_id.to_string()),
                rusqlite::types::Value::Integer(request.manifest_generation),
            ];
            bind.extend(chunk.iter().cloned().map(rusqlite::types::Value::Text));
            bind.extend(chunk.iter().cloned().map(rusqlite::types::Value::Text));
            bind.extend(
                affected_languages
                    .iter()
                    .cloned()
                    .map(rusqlite::types::Value::Text),
            );
            if pending_language {
                bind.extend(
                    affected_languages
                        .iter()
                        .cloned()
                        .map(rusqlite::types::Value::Text),
                );
            }
            let mut statement = connection.prepare(&sql)?;
            for row in statement.query_map(rusqlite::params_from_iter(bind), |row| row.get(0))? {
                versions.insert(row?);
            }
        }
    }
    Ok(versions)
}

/// Promotes when the selected-version and name/receiver identifier reads reach the
/// fixed crossover. Store predecessor phases execute both arms, including duplicate
/// reads, so the journal's changed-path count is not a safe proxy for admitted work.
fn scope_crosses_over(
    connection: &Connection,
    request: StoreDeltaScopeRequest<'_>,
    logical_recheck_file_count: usize,
    recheck_names: &BTreeSet<String>,
    selected_versions: &BTreeSet<i64>,
) -> Result<bool, ResolutionScopeError> {
    if selected_versions.is_empty() && logical_recheck_file_count == 0 {
        return Ok(false);
    }
    let total_identifiers: i64 = connection.query_row(
        "SELECT COUNT(*)
         FROM manifest_entries AS entry
         JOIN identifiers AS identifier ON identifier.version_id=entry.version_id
         WHERE entry.view_id=?1 AND entry.generation=?2
           AND entry.status IN ('indexed','failed_preserved')",
        params![request.view_id, request.manifest_generation],
        |row| row.get(0),
    )?;
    if total_identifiers == 0 {
        let total_versions: i64 = connection.query_row(
            "SELECT COUNT(*) FROM manifest_entries
             WHERE view_id=?1 AND generation=?2
               AND status IN ('indexed','failed_preserved')",
            params![request.view_id, request.manifest_generation],
            |row| row.get(0),
        )?;
        return Ok(total_versions > 0
            && logical_recheck_file_count as f64 >= total_versions as f64 * DELTA_SCOPE_CROSSOVER);
    }

    let mut scoped_identifiers = 0i64;
    let version_values = selected_versions.iter().copied().collect::<Vec<_>>();
    for chunk in version_values.chunks(SCOPE_QUERY_CHUNK) {
        let sql = format!(
            "SELECT COUNT(*) FROM identifiers WHERE version_id IN ({})",
            placeholders(chunk.len())
        );
        scoped_identifiers +=
            connection.query_row(&sql, rusqlite::params_from_iter(chunk.iter()), |row| {
                row.get::<_, i64>(0)
            })?;
    }
    for chunk in string_chunks(recheck_names) {
        let sql = format!(
            "SELECT COUNT(*)
             FROM manifest_entries AS entry
             JOIN identifiers AS identifier ON identifier.version_id=entry.version_id
             WHERE entry.view_id=? AND entry.generation=?
               AND entry.status IN ('indexed','failed_preserved')
               AND (identifier.name IN ({0})
                    OR json_extract(identifier.metadata_json,'$.receiver') IN ({0}))",
            placeholders(chunk.len())
        );
        let mut bind = vec![
            rusqlite::types::Value::Text(request.view_id.to_string()),
            rusqlite::types::Value::Integer(request.manifest_generation),
        ];
        bind.extend(chunk.iter().cloned().map(rusqlite::types::Value::Text));
        bind.extend(chunk.into_iter().map(rusqlite::types::Value::Text));
        scoped_identifiers +=
            connection.query_row(&sql, rusqlite::params_from_iter(bind), |row| {
                row.get::<_, i64>(0)
            })?;
    }
    Ok(scoped_identifiers as f64 >= total_identifiers as f64 * DELTA_SCOPE_CROSSOVER)
}

#[cfg(test)]
type ImportBinding = (String, Option<String>, Option<String>, bool, bool, bool);

#[cfg(test)]
fn import_binding(name: &str, metadata_json: Option<&str>) -> ImportBinding {
    let Some(raw) = metadata_json else {
        return (name.to_string(), None, None, false, false, false);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (name.to_string(), None, None, false, false, false);
    };
    let string_field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let local_name = string_field("alias")
        .or_else(|| string_field("local_name"))
        .unwrap_or_else(|| name.to_string());
    let imported_name = string_field("imported_name")
        .or_else(|| string_field("imported"))
        .or_else(|| string_field("importedName"))
        .or_else(|| (local_name != name).then(|| name.to_string()));
    let bool_field = |key: &str| {
        value
            .get(key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    };
    (
        local_name,
        imported_name,
        string_field("source"),
        bool_field("isTypeOnly") || bool_field("is_type_only"),
        bool_field("isDefault") || bool_field("is_default"),
        bool_field("isNamespace") || bool_field("is_namespace"),
    )
}

#[cfg(test)]
fn import_module_candidates(
    importing_path: &str,
    source: Option<&str>,
    language: &str,
) -> Vec<String> {
    let Some(source) = source else {
        return Vec::new();
    };
    if !(source.starts_with("./") || source.starts_with("../")) {
        return Vec::new();
    }
    let base = importing_path.rsplit_once('/').map_or("", |(base, _)| base);
    let mut parts = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').collect::<Vec<_>>()
    };
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Vec::new();
                }
            }
            other => parts.push(other),
        }
    }
    let module_path = parts.join("/");
    let file_name = module_path
        .rsplit_once('/')
        .map_or(module_path.as_str(), |(_, file)| file);
    if file_name.contains('.') {
        return vec![module_path];
    }
    let extensions: &[&str] = match language {
        "typescript" => &["ts", "tsx", "js", "jsx"],
        "javascript" => &["js", "jsx", "ts", "tsx"],
        _ => &[],
    };
    extensions
        .iter()
        .map(|extension| format!("{module_path}.{extension}"))
        .chain(
            extensions
                .iter()
                .map(|extension| format!("{module_path}/index.{extension}")),
        )
        .collect()
}

fn semantic_versions(versions: BTreeSet<i64>) -> Vec<SemanticVersionId> {
    versions.into_iter().map(SemanticVersionId::Store).collect()
}

fn string_chunks(values: &BTreeSet<String>) -> Vec<Vec<String>> {
    values
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .chunks(SCOPE_QUERY_CHUNK)
        .map(<[String]>::to_vec)
        .collect()
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn full(reason: StoreDeltaScopeFullReason) -> StoreDeltaScopeDecision {
    StoreDeltaScopeDecision::Full {
        worklists: ResolutionWorklists {
            scope: ResolutionWorklistScope::Corpus,
            effective_full: true,
            phase: ResolutionPhase::ResolvedPending,
            ..ResolutionWorklists::default()
        },
        reason,
    }
}
