use std::collections::{HashSet, VecDeque};

use julie_extract_artifact::resolution_store::{
    self, IdentifierWorkItem, Outcome, PendingWorkItem, ResolutionCounts, ResolutionReportRow,
    ResolutionStatus,
};
use julie_extract_artifact::writer::ResolutionScopeInput;
use rusqlite::Transaction;

pub use crate::resolution::IdentifierLocator;
use crate::resolution::{self, WorkspaceCandidateIndex};

pub const DEFAULT_RESOLUTION_WINDOW_SIZE: usize = 512;

pub trait SessionSourceKey {
    fn source_key(&self) -> &str;
}

impl SessionSourceKey for PendingWorkItem {
    fn source_key(&self) -> &str {
        &self.file_id
    }
}

impl SessionSourceKey for IdentifierWorkItem {
    fn source_key(&self) -> &str {
        &self.file_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticVersionId {
    LegacyFile(String),
    Store(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticIdentifierId {
    pub version: SemanticVersionId,
    pub local_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticPendingRelationshipId {
    pub version: SemanticVersionId,
    pub local_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticSymbolId {
    pub version: SemanticVersionId,
    pub local_id: String,
}

impl Ord for SemanticSymbolId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (&self.version, &other.version) {
            (SemanticVersionId::LegacyFile(left), SemanticVersionId::LegacyFile(right)) => self
                .local_id
                .cmp(&other.local_id)
                .then_with(|| left.cmp(right)),
            (SemanticVersionId::Store(left), SemanticVersionId::Store(right)) => left
                .cmp(right)
                .then_with(|| self.local_id.cmp(&other.local_id)),
            (SemanticVersionId::LegacyFile(_), SemanticVersionId::Store(_)) => {
                std::cmp::Ordering::Less
            }
            (SemanticVersionId::Store(_), SemanticVersionId::LegacyFile(_)) => {
                std::cmp::Ordering::Greater
            }
        }
    }
}

impl PartialOrd for SemanticSymbolId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionCorpusIdentity {
    Legacy {
        revision: i64,
    },
    Store {
        family_id: String,
        view_id: String,
        manifest_generation: i64,
        manifest_hash: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionResolutionState {
    pub status: ResolutionStatus,
    pub version: i64,
    pub last_full_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPassRequest {
    pub full: bool,
}

impl ResolutionPassRequest {
    pub fn full() -> Self {
        Self { full: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ResolutionWorklistScope {
    #[default]
    Corpus,
    Versions(Vec<SemanticVersionId>),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolutionWorklists {
    pub scope: ResolutionWorklistScope,
    pub effective_full: bool,
    pub recheck_names: Vec<String>,
    pub recheck_versions: Vec<SemanticVersionId>,
    pub selected_versions: Vec<SemanticVersionId>,
    pub changed_versions: Vec<SemanticVersionId>,
    pub phase: ResolutionPhase,
    pub repair_identifiers: Vec<(SemanticIdentifierId, String)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResolutionPhase {
    #[default]
    ResolvedPending,
    PropagationCovered,
    ResolvedIdentifiers,
    Pending,
    Relationships,
    Identifiers,
    PropagationOwned,
    WorkspaceGated,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRelationship {
    pub target_symbol_id: SemanticSymbolId,
    pub source_version_id: SemanticVersionId,
    pub kind: String,
    pub start_line: i64,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionResolvedPendingWorkItem {
    pub pending: PendingWorkItem,
    pub target_symbol_id: SemanticSymbolId,
    pub tier: i64,
    pub confidence: f64,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionResolvedIdentifierWorkItem {
    pub identifier: IdentifierWorkItem,
    pub target_symbol_id: Option<SemanticSymbolId>,
    pub tier: Option<i64>,
    pub confidence: Option<f64>,
    pub method: Option<String>,
    pub outcome: Outcome,
    pub candidates: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CurrentResolutionOverlay {
    pub resolved_pending: Vec<SessionResolvedPendingWorkItem>,
    pub resolved_identifiers: Vec<SessionResolvedIdentifierWorkItem>,
    pub pending: Vec<PendingWorkItem>,
    pub identifiers: Vec<IdentifierWorkItem>,
    pub relationships: Vec<SessionRelationship>,
    pub identifier_ids: HashSet<SemanticIdentifierId>,
    pub gated_languages: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionPhaseChunk {
    ResolvedPending(Vec<SessionResolvedPendingWorkItem>),
    ResolvedIdentifiers(Vec<SessionResolvedIdentifierWorkItem>),
    Pending(Vec<PendingWorkItem>),
    Relationships(Vec<SessionRelationship>),
    Identifiers(Vec<IdentifierWorkItem>),
    WorkspaceGated(std::collections::BTreeSet<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionWrite {
    Pending {
        pending_relationship_id: SemanticPendingRelationshipId,
        target_symbol_id: SemanticSymbolId,
        tier: u8,
        confidence: f64,
        method: String,
        revision: i64,
    },
    DemotePending {
        pending_relationship_id: SemanticPendingRelationshipId,
    },
    Identifier {
        identifier_id: SemanticIdentifierId,
        target_symbol_id: Option<SemanticSymbolId>,
        outcome: Outcome,
        tier: Option<u8>,
        confidence: Option<f64>,
        method: Option<String>,
        candidates: Option<i64>,
        revision: i64,
    },
    DemoteIdentifier {
        identifier_id: SemanticIdentifierId,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolutionWriteBatch {
    pub writes: Vec<ResolutionWrite>,
}

impl ResolutionWriteBatch {
    pub fn record_pending_resolution(
        &mut self,
        pending_relationship_id: SemanticPendingRelationshipId,
        target_symbol_id: SemanticSymbolId,
        tier: u8,
        confidence: f64,
        method: &str,
        revision: i64,
    ) {
        self.writes.push(ResolutionWrite::Pending {
            pending_relationship_id,
            target_symbol_id,
            tier,
            confidence,
            method: method.to_string(),
            revision,
        });
    }

    pub fn demote_pending(&mut self, pending_relationship_id: SemanticPendingRelationshipId) {
        self.writes.push(ResolutionWrite::DemotePending {
            pending_relationship_id,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_identifier_outcome(
        &mut self,
        identifier_id: SemanticIdentifierId,
        outcome: Outcome,
        target_symbol_id: Option<SemanticSymbolId>,
        tier: Option<u8>,
        confidence: Option<f64>,
        method: Option<&str>,
        candidates: Option<i64>,
        revision: i64,
    ) {
        self.writes.push(ResolutionWrite::Identifier {
            identifier_id,
            target_symbol_id,
            outcome,
            tier,
            confidence,
            method: method.map(str::to_string),
            candidates,
            revision,
        });
    }

    pub fn demote_identifier(&mut self, identifier_id: SemanticIdentifierId) {
        self.writes
            .push(ResolutionWrite::DemoteIdentifier { identifier_id });
    }
}

pub trait ResolutionSession {
    type Error;

    fn corpus_identity(&self) -> Result<ResolutionCorpusIdentity, Self::Error>;
    fn prior_resolution_state(&mut self) -> Result<Option<SessionResolutionState>, Self::Error>;
    fn current_revision(&mut self) -> Result<i64, Self::Error>;
    fn open_resolution_pass(
        &mut self,
        request: &ResolutionPassRequest,
    ) -> Result<ResolutionWorklists, Self::Error>;
    fn qualify_version(&self, source_key: &str) -> Result<SemanticVersionId, Self::Error>;
    fn resolve_edge(
        &mut self,
        edge: &resolution::UnresolvedEdge,
    ) -> Result<resolution::TierOutcome, Self::Error>;
    fn target_symbol_name(
        &mut self,
        symbol_id: &SemanticSymbolId,
    ) -> Result<Option<String>, Self::Error>;
    #[allow(clippy::too_many_arguments)]
    fn locate_identifier(
        &self,
        version: &SemanticVersionId,
        name: &str,
        start_byte: Option<i64>,
        end_byte: Option<i64>,
        start_line: i64,
    ) -> Result<Option<String>, Self::Error>;
    fn identifier_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error>;
    fn propagation_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error>;
    fn propagation_is_owned(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error>;
    fn next_phase_chunk(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<Option<ResolutionPhaseChunk>, Self::Error>;
    fn flush(&mut self, writes: ResolutionWriteBatch) -> Result<ResolutionCounts, Self::Error>;
    fn aggregate_report(&mut self) -> Result<Vec<ResolutionReportRow>, Self::Error>;
    fn prepare_shadow(
        &mut self,
        worklists: &ResolutionWorklists,
        revision: i64,
    ) -> Result<(), Self::Error> {
        let _ = (worklists, revision);
        Ok(())
    }
    fn verify_shadow(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub struct LegacyResolutionSession<'session, 'connection> {
    transaction: &'session Transaction<'connection>,
    scope: &'session ResolutionScopeInput,
    crossover: f64,
    shadow_baseline: Option<resolution::OverlaySnapshot>,
    index: Option<WorkspaceCandidateIndex>,
    locator: Option<IdentifierLocator>,
    covered: HashSet<SemanticIdentifierId>,
    propagation_covered: Option<HashSet<SemanticIdentifierId>>,
    propagation_owned: Option<HashSet<SemanticIdentifierId>>,
    phase: Option<ResolutionPhase>,
    phase_chunks: VecDeque<ResolutionPhaseChunk>,
    active_worklists: Option<ResolutionWorklists>,
    window_size: usize,
    max_emitted_chunk_size: usize,
}

impl<'session, 'connection> LegacyResolutionSession<'session, 'connection> {
    pub fn new(
        transaction: &'session Transaction<'connection>,
        scope: &'session ResolutionScopeInput,
        crossover: f64,
    ) -> Self {
        Self {
            transaction,
            scope,
            crossover,
            shadow_baseline: None,
            index: None,
            locator: None,
            covered: HashSet::new(),
            propagation_covered: None,
            propagation_owned: None,
            phase: None,
            phase_chunks: VecDeque::new(),
            active_worklists: None,
            window_size: DEFAULT_RESOLUTION_WINDOW_SIZE,
            max_emitted_chunk_size: 0,
        }
    }

    pub fn with_window_size(mut self, window_size: usize) -> Self {
        self.window_size = window_size.max(1);
        self
    }

    pub fn max_emitted_chunk_size(&self) -> usize {
        self.max_emitted_chunk_size
    }

    fn pop_phase_chunk(&mut self) -> Option<ResolutionPhaseChunk> {
        let chunk = self.phase_chunks.pop_front()?;
        let len = match &chunk {
            ResolutionPhaseChunk::ResolvedPending(rows) => rows.len(),
            ResolutionPhaseChunk::ResolvedIdentifiers(rows) => rows.len(),
            ResolutionPhaseChunk::Pending(rows) => rows.len(),
            ResolutionPhaseChunk::Relationships(rows) => rows.len(),
            ResolutionPhaseChunk::Identifiers(rows) => rows.len(),
            ResolutionPhaseChunk::WorkspaceGated(rows) => rows.len(),
        };
        self.max_emitted_chunk_size = self.max_emitted_chunk_size.max(len);
        Some(chunk)
    }

    pub(crate) fn seed_pass(
        &mut self,
        index: WorkspaceCandidateIndex,
        locator: IdentifierLocator,
        worklists: &ResolutionWorklists,
    ) -> Result<(), rusqlite::Error> {
        let files = Self::legacy_files(&worklists.scope);
        let covered =
            resolution::covered_identifiers(self.transaction, &index, &locator, files.as_deref())?;
        self.covered = self.semantic_identifier_ids(covered)?;
        self.index = Some(index);
        self.locator = Some(locator);
        self.active_worklists = Some(worklists.clone());
        self.phase = None;
        Ok(())
    }

    fn legacy_files(scope: &ResolutionWorklistScope) -> Option<Vec<&str>> {
        match scope {
            ResolutionWorklistScope::Corpus => None,
            ResolutionWorklistScope::Versions(versions) => Some(
                versions
                    .iter()
                    .filter_map(|version| match version {
                        SemanticVersionId::LegacyFile(local_id) => Some(local_id.as_str()),
                        SemanticVersionId::Store(_) => None,
                    })
                    .collect(),
            ),
        }
    }

    fn legacy_version_refs(versions: &[SemanticVersionId]) -> Vec<&str> {
        versions
            .iter()
            .filter_map(|version| match version {
                SemanticVersionId::LegacyFile(local_id) => Some(local_id.as_str()),
                SemanticVersionId::Store(_) => None,
            })
            .collect()
    }

    fn semantic_identifier_ids(
        &self,
        local_ids: impl IntoIterator<Item = String>,
    ) -> Result<HashSet<SemanticIdentifierId>, rusqlite::Error> {
        let local_ids: Vec<String> = local_ids.into_iter().collect();
        let mut semantic_ids = HashSet::new();
        for chunk in local_ids.chunks(900) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT identifier_id, file_id FROM identifiers \
                 WHERE identifier_id IN ({placeholders})"
            );
            let mut statement = self.transaction.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (local_id, source_id) = row?;
                semantic_ids.insert(SemanticIdentifierId {
                    version: SemanticVersionId::LegacyFile(source_id),
                    local_id,
                });
            }
        }
        Ok(semantic_ids)
    }

    fn semantic_resolved_pending(
        index: &WorkspaceCandidateIndex,
        items: Vec<resolution_store::ResolvedPendingWorkItem>,
    ) -> Result<Vec<SessionResolvedPendingWorkItem>, rusqlite::Error> {
        items
            .into_iter()
            .map(|item| {
                Ok(SessionResolvedPendingWorkItem {
                    target_symbol_id: index
                        .semantic_id_by_local(&item.target_symbol_id)
                        .ok_or(rusqlite::Error::QueryReturnedNoRows)?,
                    pending: item.pending,
                    tier: item.tier,
                    confidence: item.confidence,
                    method: item.method,
                })
            })
            .collect()
    }

    fn semantic_resolved_identifiers(
        index: &WorkspaceCandidateIndex,
        items: Vec<resolution_store::ResolvedIdentifierWorkItem>,
    ) -> Result<Vec<SessionResolvedIdentifierWorkItem>, rusqlite::Error> {
        items
            .into_iter()
            .map(|item| {
                Ok(SessionResolvedIdentifierWorkItem {
                    target_symbol_id: item
                        .target_symbol_id
                        .as_deref()
                        .map(|id| {
                            index
                                .semantic_id_by_local(id)
                                .ok_or(rusqlite::Error::QueryReturnedNoRows)
                        })
                        .transpose()?,
                    identifier: item.identifier,
                    tier: item.tier,
                    confidence: item.confidence,
                    method: item.method,
                    outcome: item.outcome,
                    candidates: item.candidates,
                })
            })
            .collect()
    }
}

impl ResolutionSession for LegacyResolutionSession<'_, '_> {
    type Error = rusqlite::Error;

    fn corpus_identity(&self) -> Result<ResolutionCorpusIdentity, Self::Error> {
        Ok(ResolutionCorpusIdentity::Legacy {
            revision: resolution::current_revision(self.transaction)?,
        })
    }

    fn prior_resolution_state(&mut self) -> Result<Option<SessionResolutionState>, Self::Error> {
        Ok(
            resolution_store::read_resolution_metadata(self.transaction)?.map(|state| {
                SessionResolutionState {
                    status: state.status,
                    version: state.version,
                    last_full_revision: state.last_full_revision,
                }
            }),
        )
    }

    fn current_revision(&mut self) -> Result<i64, Self::Error> {
        resolution::current_revision(self.transaction)
    }

    fn open_resolution_pass(
        &mut self,
        request: &ResolutionPassRequest,
    ) -> Result<ResolutionWorklists, Self::Error> {
        let index = resolution::load_index(self.transaction)?;
        let (scope, effective_full, recheck_names, recheck_versions, selected_versions) =
            if request.full {
                (
                    ResolutionWorklistScope::Corpus,
                    true,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            } else {
                let revision = resolution::current_revision(self.transaction)?;
                let delta =
                    resolution::delta_scope_files(self.transaction, self.scope, &index, revision)?;
                let effective_full = resolution::delta_scope_crosses_over(
                    self.transaction,
                    self.scope.changed_file_ids.len(),
                    &delta,
                    self.crossover,
                )?;
                if effective_full {
                    (
                        ResolutionWorklistScope::Corpus,
                        true,
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                    )
                } else {
                    let selected_versions: Vec<SemanticVersionId> = delta
                        .selected_row_files
                        .iter()
                        .cloned()
                        .map(SemanticVersionId::LegacyFile)
                        .collect();
                    let recheck_versions: Vec<SemanticVersionId> = delta
                        .recheck_files
                        .iter()
                        .cloned()
                        .map(SemanticVersionId::LegacyFile)
                        .collect();
                    (
                        ResolutionWorklistScope::Versions(selected_versions.clone()),
                        false,
                        delta.recheck_names,
                        recheck_versions,
                        selected_versions,
                    )
                }
            };
        let changed_versions = self
            .scope
            .changed_file_ids
            .iter()
            .cloned()
            .map(SemanticVersionId::LegacyFile)
            .collect();
        let worklists = ResolutionWorklists {
            scope,
            effective_full,
            recheck_names,
            recheck_versions,
            selected_versions,
            changed_versions,
            phase: ResolutionPhase::ResolvedPending,
            repair_identifiers: Vec::new(),
        };
        let files = Self::legacy_files(&worklists.scope);
        let locator = IdentifierLocator::load_scoped(self.transaction, files.as_deref())?;
        let covered =
            resolution::covered_identifiers(self.transaction, &index, &locator, files.as_deref())?;
        self.covered = self.semantic_identifier_ids(covered)?;
        self.index = Some(index);
        self.locator = Some(locator);
        self.propagation_covered = None;
        self.propagation_owned = None;
        self.phase = None;
        self.phase_chunks.clear();
        self.active_worklists = Some(worklists.clone());
        Ok(worklists)
    }

    fn qualify_version(&self, source_key: &str) -> Result<SemanticVersionId, Self::Error> {
        Ok(SemanticVersionId::LegacyFile(source_key.to_string()))
    }

    fn resolve_edge(
        &mut self,
        edge: &resolution::UnresolvedEdge,
    ) -> Result<resolution::TierOutcome, Self::Error> {
        let result = resolution::resolve_with_candidate_lookup(
            self.index.as_ref().ok_or(rusqlite::Error::InvalidQuery)?,
            edge,
        );
        Ok(result.unwrap_or_else(|never| match never {}))
    }

    fn target_symbol_name(
        &mut self,
        symbol_id: &SemanticSymbolId,
    ) -> Result<Option<String>, Self::Error> {
        Ok(self
            .index
            .as_ref()
            .and_then(|index| index.symbol_name(symbol_id))
            .map(str::to_string))
    }

    fn locate_identifier(
        &self,
        version: &SemanticVersionId,
        name: &str,
        start_byte: Option<i64>,
        end_byte: Option<i64>,
        start_line: i64,
    ) -> Result<Option<String>, Self::Error> {
        let SemanticVersionId::LegacyFile(source_key) = version else {
            return Ok(None);
        };
        Ok(self
            .locator
            .as_ref()
            .and_then(|locator| locator.locate(source_key, name, start_byte, end_byte, start_line)))
    }

    fn identifier_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        Ok(self.covered.contains(identifier_id))
    }

    fn propagation_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        if self.propagation_covered.is_none() {
            let worklists = self
                .active_worklists
                .as_ref()
                .ok_or(rusqlite::Error::InvalidQuery)?;
            let files = (!worklists.effective_full)
                .then(|| Self::legacy_version_refs(&worklists.selected_versions));
            let local_ids = resolution::propagation_covered_identifiers(
                self.transaction,
                self.index.as_ref().ok_or(rusqlite::Error::InvalidQuery)?,
                self.locator.as_ref().ok_or(rusqlite::Error::InvalidQuery)?,
                files.as_deref(),
            )?;
            self.propagation_covered = Some(self.semantic_identifier_ids(local_ids)?);
        }
        Ok(self
            .propagation_covered
            .as_ref()
            .is_some_and(|ids| ids.contains(identifier_id)))
    }

    fn propagation_is_owned(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        if self.propagation_owned.is_none() {
            let raw_covered: HashSet<String> =
                self.covered.iter().map(|id| id.local_id.clone()).collect();
            let local_ids =
                resolution::propagation_owned_identifiers(self.transaction, &raw_covered)?;
            self.propagation_owned = Some(self.semantic_identifier_ids(local_ids)?);
        }
        Ok(self
            .propagation_owned
            .as_ref()
            .is_some_and(|ids| ids.contains(identifier_id)))
    }

    fn next_phase_chunk(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<Option<ResolutionPhaseChunk>, Self::Error> {
        if self.phase == Some(worklists.phase) {
            return Ok(self.pop_phase_chunk());
        }
        let index = self.index.as_ref().ok_or(rusqlite::Error::InvalidQuery)?;
        let locator = self.locator.as_ref().ok_or(rusqlite::Error::InvalidQuery)?;
        let covered = &self.covered;
        let mut overlay = CurrentResolutionOverlay::default();
        let names: Vec<&str> = worklists.recheck_names.iter().map(String::as_str).collect();
        let recheck_files = Self::legacy_version_refs(&worklists.recheck_versions);
        let selected_files = Self::legacy_version_refs(&worklists.selected_versions);
        let changed_files = Self::legacy_version_refs(&worklists.changed_versions);
        match worklists.phase {
            ResolutionPhase::ResolvedPending => {
                let items = if worklists.effective_full {
                    resolution_store::worklist_resolved_pending(self.transaction)?
                } else {
                    resolution::merge_by_key(
                        resolution_store::worklist_resolved_pending_by_names(
                            self.transaction,
                            &names,
                        )?,
                        resolution_store::worklist_resolved_pending_in_files(
                            self.transaction,
                            &recheck_files,
                        )?,
                        |item| item.pending.pending_relationship_id.clone(),
                    )
                };
                overlay.resolved_pending = Self::semantic_resolved_pending(index, items)?;
            }
            ResolutionPhase::PropagationCovered => {
                let files = (!worklists.effective_full).then_some(selected_files.as_slice());
                let local_ids = resolution::propagation_covered_identifiers(
                    self.transaction,
                    index,
                    locator,
                    files,
                )?;
                overlay.identifier_ids = self.semantic_identifier_ids(local_ids)?;
            }
            ResolutionPhase::ResolvedIdentifiers => {
                let items = if worklists.effective_full {
                    resolution_store::worklist_resolved_identifiers(self.transaction)?
                } else {
                    resolution::merge_by_key(
                        resolution_store::worklist_resolved_identifiers_by_names(
                            self.transaction,
                            &names,
                        )?,
                        resolution_store::worklist_resolved_identifiers_in_files(
                            self.transaction,
                            &recheck_files,
                        )?,
                        |item| item.identifier.identifier_id.clone(),
                    )
                };
                overlay.resolved_identifiers = Self::semantic_resolved_identifiers(index, items)?;
            }
            ResolutionPhase::Pending => {
                overlay.pending = if worklists.effective_full {
                    resolution_store::worklist_full_pending(self.transaction)?
                } else {
                    resolution::merge_by_key(
                        resolution_store::worklist_unresolved_pending_by_names(
                            self.transaction,
                            &names,
                        )?,
                        resolution::unresolved_pending_in_files(self.transaction, &recheck_files)?,
                        |item| item.pending_relationship_id.clone(),
                    )
                };
            }
            ResolutionPhase::Relationships => {
                let files = (!worklists.effective_full).then_some(changed_files.as_slice());
                overlay.relationships =
                    resolution::load_relationship_rows(self.transaction, files)?
                        .into_iter()
                        .map(|row| SessionRelationship {
                            target_symbol_id: SemanticSymbolId {
                                version: SemanticVersionId::LegacyFile(row.target_source_key),
                                local_id: row.target_symbol_id,
                            },
                            source_version_id: SemanticVersionId::LegacyFile(row.source_key),
                            kind: row.kind,
                            start_line: row.start_line,
                            start_byte: row.start_byte,
                            end_byte: row.end_byte,
                            confidence: row.confidence,
                        })
                        .collect();
            }
            ResolutionPhase::Identifiers => {
                overlay.identifiers = if worklists.effective_full {
                    resolution_store::worklist_full_identifiers(self.transaction)?
                } else {
                    let mut identifiers = resolution::merge_by_key(
                        resolution_store::worklist_never_attempted_identifiers_by_names(
                            self.transaction,
                            &names,
                        )?,
                        resolution_store::worklist_never_attempted_identifiers_by_files(
                            self.transaction,
                            &recheck_files,
                        )?,
                        |item| item.identifier_id.clone(),
                    );
                    let repair_names: Vec<&str> = worklists
                        .repair_identifiers
                        .iter()
                        .map(|(_, name)| name.as_str())
                        .collect();
                    let repair_ids: HashSet<&str> = worklists
                        .repair_identifiers
                        .iter()
                        .map(|(id, _)| id.local_id.as_str())
                        .collect();
                    let mut seen: HashSet<String> = identifiers
                        .iter()
                        .map(|item| item.identifier_id.clone())
                        .collect();
                    for item in resolution_store::worklist_never_attempted_identifiers_by_names(
                        self.transaction,
                        &repair_names,
                    )? {
                        if repair_ids.contains(item.identifier_id.as_str())
                            && seen.insert(item.identifier_id.clone())
                        {
                            identifiers.push(item);
                        }
                    }
                    identifiers
                };
            }
            ResolutionPhase::PropagationOwned => {
                let raw_covered: HashSet<String> =
                    covered.iter().map(|id| id.local_id.clone()).collect();
                let local_ids =
                    resolution::propagation_owned_identifiers(self.transaction, &raw_covered)?;
                overlay.identifier_ids = self.semantic_identifier_ids(local_ids)?;
            }
            ResolutionPhase::WorkspaceGated => {
                overlay.gated_languages =
                    resolution::workspace_tier2_gated_languages(self.transaction)?;
            }
        }
        let chunk = match worklists.phase {
            ResolutionPhase::ResolvedPending => {
                ResolutionPhaseChunk::ResolvedPending(overlay.resolved_pending)
            }
            ResolutionPhase::ResolvedIdentifiers => {
                ResolutionPhaseChunk::ResolvedIdentifiers(overlay.resolved_identifiers)
            }
            ResolutionPhase::Pending => ResolutionPhaseChunk::Pending(overlay.pending),
            ResolutionPhase::Relationships => {
                ResolutionPhaseChunk::Relationships(overlay.relationships)
            }
            ResolutionPhase::Identifiers => ResolutionPhaseChunk::Identifiers(overlay.identifiers),
            ResolutionPhase::WorkspaceGated => {
                ResolutionPhaseChunk::WorkspaceGated(overlay.gated_languages)
            }
            ResolutionPhase::PropagationCovered | ResolutionPhase::PropagationOwned => {
                return Ok(None);
            }
        };
        self.phase = Some(worklists.phase);
        self.phase_chunks.clear();
        match chunk {
            ResolutionPhaseChunk::ResolvedPending(rows) => self.phase_chunks.extend(
                rows.chunks(self.window_size)
                    .map(|chunk| ResolutionPhaseChunk::ResolvedPending(chunk.to_vec())),
            ),
            ResolutionPhaseChunk::ResolvedIdentifiers(rows) => self.phase_chunks.extend(
                rows.chunks(self.window_size)
                    .map(|chunk| ResolutionPhaseChunk::ResolvedIdentifiers(chunk.to_vec())),
            ),
            ResolutionPhaseChunk::Pending(rows) => self.phase_chunks.extend(
                rows.chunks(self.window_size)
                    .map(|chunk| ResolutionPhaseChunk::Pending(chunk.to_vec())),
            ),
            ResolutionPhaseChunk::Relationships(rows) => self.phase_chunks.extend(
                rows.chunks(self.window_size)
                    .map(|chunk| ResolutionPhaseChunk::Relationships(chunk.to_vec())),
            ),
            ResolutionPhaseChunk::Identifiers(rows) => self.phase_chunks.extend(
                rows.chunks(self.window_size)
                    .map(|chunk| ResolutionPhaseChunk::Identifiers(chunk.to_vec())),
            ),
            ResolutionPhaseChunk::WorkspaceGated(rows) => self
                .phase_chunks
                .push_back(ResolutionPhaseChunk::WorkspaceGated(rows)),
        }
        Ok(self.pop_phase_chunk())
    }

    fn flush(&mut self, writes: ResolutionWriteBatch) -> Result<ResolutionCounts, Self::Error> {
        let mut buffer = resolution_store::ResolutionWriteBuffer::new();
        for write in writes.writes {
            match write {
                ResolutionWrite::Pending {
                    pending_relationship_id,
                    target_symbol_id,
                    tier,
                    confidence,
                    method,
                    revision,
                } => buffer.record_pending_resolution(
                    &pending_relationship_id.local_id,
                    &target_symbol_id.local_id,
                    tier,
                    confidence,
                    &method,
                    revision,
                ),
                ResolutionWrite::DemotePending {
                    pending_relationship_id,
                } => buffer.demote_pending(&pending_relationship_id.local_id),
                ResolutionWrite::Identifier {
                    identifier_id,
                    target_symbol_id,
                    outcome,
                    tier,
                    confidence,
                    method,
                    candidates,
                    revision,
                } => buffer.record_identifier_outcome(
                    &identifier_id.local_id,
                    outcome,
                    target_symbol_id.as_ref().map(|id| id.local_id.as_str()),
                    tier,
                    confidence,
                    method.as_deref(),
                    candidates,
                    revision,
                ),
                ResolutionWrite::DemoteIdentifier { identifier_id } => {
                    buffer.demote_identifier(&identifier_id.local_id);
                }
            }
        }
        buffer.flush(self.transaction)?;
        self.propagation_covered = None;
        self.propagation_owned = None;
        Ok(ResolutionCounts::default())
    }

    fn aggregate_report(&mut self) -> Result<Vec<ResolutionReportRow>, Self::Error> {
        resolution_store::resolution_report(self.transaction)
    }

    fn prepare_shadow(
        &mut self,
        worklists: &ResolutionWorklists,
        revision: i64,
    ) -> Result<(), Self::Error> {
        if !worklists.effective_full {
            self.shadow_baseline = resolution::capture_legacy_shadow(
                self.transaction,
                self.scope,
                self.index.as_ref().ok_or(rusqlite::Error::InvalidQuery)?,
                revision,
            )?;
        }
        Ok(())
    }

    fn verify_shadow(&mut self) -> Result<(), Self::Error> {
        if let Some(baseline) = self.shadow_baseline.take() {
            resolution::verify_legacy_shadow(self.transaction, &baseline)?;
        }
        Ok(())
    }
}
