use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::model::{ArtifactFile, FileStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreLevel {
    L1,
    L2,
    L3,
}

impl StoreLevel {
    pub fn as_i64(self) -> i64 {
        match self {
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreProjectionError {
    FileNotIndexed(FileStatus),
    MissingContentHash,
    InvalidExtractionEpoch(u32),
    NegativeContentBytes(i64),
    NegativeLineCount(i64),
}

impl fmt::Display for StoreProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotIndexed(status) => {
                write!(
                    formatter,
                    "store versions require indexed files, found {}",
                    status.as_str()
                )
            }
            Self::MissingContentHash => {
                formatter.write_str("store versions require a content hash")
            }
            Self::InvalidExtractionEpoch(epoch) => {
                write!(
                    formatter,
                    "extraction epoch must be positive, found {epoch}"
                )
            }
            Self::NegativeContentBytes(bytes) => {
                write!(
                    formatter,
                    "content bytes must be non-negative, found {bytes}"
                )
            }
            Self::NegativeLineCount(lines) => {
                write!(formatter, "line count must be non-negative, found {lines}")
            }
        }
    }
}

impl Error for StoreProjectionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreReferenceSite {
    pub reference_site_id: String,
    pub path: String,
    pub language: String,
    pub containing_symbol_id: Option<String>,
    pub start_line: Option<i64>,
    pub start_column: Option<i64>,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub is_exact: bool,
    pub provenance: String,
    pub level: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreRowCounts {
    pub file_versions: i64,
    pub parser_inventory: i64,
    pub language_capabilities: i64,
    pub language_capability_fixtures: i64,
    pub language_capability_gaps: i64,
    pub symbols: i64,
    pub symbol_annotations: i64,
    pub reference_sites: i64,
    pub identifiers: i64,
    pub relationships: i64,
    pub pending_relationships: i64,
    pub type_facts: i64,
    pub type_argument_usages: i64,
    pub type_arguments: i64,
    pub literals: i64,
    pub source_regions: i64,
    pub structural_facts: i64,
    pub complexity_metrics: i64,
    pub parse_diagnostics: i64,
}

impl StoreRowCounts {
    pub fn total(self) -> i64 {
        self.file_versions
            + self.parser_inventory
            + self.language_capabilities
            + self.language_capability_fixtures
            + self.language_capability_gaps
            + self.symbols
            + self.symbol_annotations
            + self.reference_sites
            + self.identifiers
            + self.relationships
            + self.pending_relationships
            + self.type_facts
            + self.type_argument_usages
            + self.type_arguments
            + self.literals
            + self.source_regions
            + self.structural_facts
            + self.complexity_metrics
            + self.parse_diagnostics
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoreFileVersion {
    extraction_epoch: u32,
    file: ArtifactFile,
    l1_reference_sites: Vec<StoreReferenceSite>,
    l2_reference_sites: Vec<StoreReferenceSite>,
}

impl StoreFileVersion {
    pub fn try_from_artifact_file(
        extraction_epoch: u32,
        file: &ArtifactFile,
    ) -> Result<Self, StoreProjectionError> {
        if file.status != FileStatus::Indexed {
            return Err(StoreProjectionError::FileNotIndexed(file.status));
        }
        if file.content_hash.is_empty() {
            return Err(StoreProjectionError::MissingContentHash);
        }
        if extraction_epoch == 0 {
            return Err(StoreProjectionError::InvalidExtractionEpoch(
                extraction_epoch,
            ));
        }
        if file.content_bytes < 0 {
            return Err(StoreProjectionError::NegativeContentBytes(
                file.content_bytes,
            ));
        }
        if let Some(line_count) = file.line_count.filter(|count| *count < 0) {
            return Err(StoreProjectionError::NegativeLineCount(line_count));
        }

        let mut file = file.clone();
        retain_version_local_rows(&mut file);
        let (l1_reference_sites, l2_reference_sites) = project_reference_sites(&file);
        Ok(Self {
            extraction_epoch,
            file,
            l1_reference_sites,
            l2_reference_sites,
        })
    }

    pub fn extraction_epoch(&self) -> u32 {
        self.extraction_epoch
    }

    pub fn path(&self) -> &str {
        &self.file.path
    }

    pub fn content_hash(&self) -> &str {
        &self.file.content_hash
    }

    pub fn artifact_file(&self) -> &ArtifactFile {
        &self.file
    }

    pub fn file_metadata_json(&self) -> Option<&str> {
        self.file.metadata_json.as_deref()
    }

    pub fn reference_sites(&self, level: StoreLevel) -> &[StoreReferenceSite] {
        match level {
            StoreLevel::L1 => &self.l1_reference_sites,
            StoreLevel::L2 => &self.l2_reference_sites,
            StoreLevel::L3 => &[],
        }
    }

    pub fn row_counts(&self, level: StoreLevel) -> StoreRowCounts {
        let file = &self.file;
        match level {
            StoreLevel::L1 => StoreRowCounts {
                file_versions: 1,
                symbols: file.symbols.len() as i64,
                symbol_annotations: file.symbol_annotations.len() as i64,
                reference_sites: self.l1_reference_sites.len() as i64,
                relationships: file.relationships.len() as i64,
                pending_relationships: file.pending_relationships.len() as i64,
                type_facts: file.type_facts.len() as i64,
                complexity_metrics: file.complexity_metrics.len() as i64,
                parse_diagnostics: file.parse_diagnostics.len() as i64,
                ..StoreRowCounts::default()
            },
            StoreLevel::L2 => StoreRowCounts {
                reference_sites: self.l2_reference_sites.len() as i64,
                identifiers: file.identifiers.len() as i64,
                ..StoreRowCounts::default()
            },
            StoreLevel::L3 => StoreRowCounts {
                type_argument_usages: file.type_argument_usages.len() as i64,
                type_arguments: file.type_arguments.len() as i64,
                literals: file.literals.len() as i64,
                source_regions: file.source_regions.len() as i64,
                structural_facts: file.structural_facts.len() as i64,
                ..StoreRowCounts::default()
            },
        }
    }

    pub fn l1_projection_equals(&self, other: &Self) -> bool {
        let left = &self.file;
        let right = &other.file;
        self.extraction_epoch == other.extraction_epoch
            && left.path == right.path
            && left.content_hash == right.content_hash
            && left.language == right.language
            && left.content_bytes == right.content_bytes
            && left.line_count == right.line_count
            && left.metadata_json == right.metadata_json
            && left.symbols == right.symbols
            && left.symbol_annotations == right.symbol_annotations
            && left.relationships == right.relationships
            && left.pending_relationships == right.pending_relationships
            && left.type_facts == right.type_facts
            && left.complexity_metrics == right.complexity_metrics
            && left.parse_diagnostics == right.parse_diagnostics
            && self.l1_reference_sites == other.l1_reference_sites
    }
}

fn retain_version_local_rows(file: &mut ArtifactFile) {
    let symbol_ids = file
        .symbols
        .iter()
        .map(|symbol| symbol.symbol_id.clone())
        .collect::<HashSet<_>>();
    for symbol in &mut file.symbols {
        if !option_is_present(&symbol_ids, symbol.parent_symbol_id.as_deref()) {
            symbol.parent_symbol_id = None;
        }
    }
    file.symbol_annotations
        .retain(|annotation| symbol_ids.contains(&annotation.symbol_id));
    for identifier in &mut file.identifiers {
        if !option_is_present(&symbol_ids, identifier.containing_symbol_id.as_deref()) {
            identifier.containing_symbol_id = None;
        }
    }
    file.relationships.retain(|relationship| {
        symbol_ids.contains(&relationship.from_symbol_id)
            && symbol_ids.contains(&relationship.to_symbol_id)
    });
    file.pending_relationships
        .retain(|pending| symbol_ids.contains(&pending.from_symbol_id));
    for pending in &mut file.pending_relationships {
        if !option_is_present(&symbol_ids, pending.caller_scope_symbol_id.as_deref()) {
            pending.caller_scope_symbol_id = None;
        }
    }
    file.type_facts
        .retain(|fact| symbol_ids.contains(&fact.symbol_id));

    let identifier_ids = file
        .identifiers
        .iter()
        .map(|identifier| identifier.identifier_id.clone())
        .collect::<HashSet<_>>();
    file.type_argument_usages
        .retain(|usage| identifier_ids.contains(&usage.identifier_id));
    let usage_ids = file
        .type_argument_usages
        .iter()
        .map(|usage| usage.usage_id.clone())
        .collect::<HashSet<_>>();
    file.type_arguments
        .retain(|argument| usage_ids.contains(&argument.usage_id));

    for literal in &mut file.literals {
        if !option_is_present(&symbol_ids, literal.containing_symbol_id.as_deref()) {
            literal.containing_symbol_id = None;
        }
    }
    for region in &mut file.source_regions {
        if !option_is_present(&symbol_ids, region.containing_symbol_id.as_deref()) {
            region.containing_symbol_id = None;
        }
    }
    for fact in &mut file.structural_facts {
        if !option_is_present(&symbol_ids, fact.containing_symbol_id.as_deref()) {
            fact.containing_symbol_id = None;
        }
    }
    for metric in &mut file.complexity_metrics {
        if !option_is_present(&symbol_ids, metric.symbol_id.as_deref()) {
            metric.symbol_id = None;
        }
    }
}

fn option_is_present(ids: &HashSet<String>, value: Option<&str>) -> bool {
    value.is_none_or(|value| ids.contains(value))
}

fn project_reference_sites(
    file: &ArtifactFile,
) -> (Vec<StoreReferenceSite>, Vec<StoreReferenceSite>) {
    let l1_claims = file
        .relationships
        .iter()
        .map(|row| row.reference_site_id.as_str())
        .chain(
            file.pending_relationships
                .iter()
                .map(|row| row.reference_site_id.as_str()),
        )
        .collect::<HashSet<_>>();
    let mut sites = Vec::new();
    let mut indexes = HashMap::new();

    for identifier in &file.identifiers {
        let exact = identifier.site_is_exact;
        insert_first_site(
            &mut sites,
            &mut indexes,
            StoreReferenceSite {
                reference_site_id: identifier.reference_site_id.clone(),
                path: file.path.clone(),
                language: file.language.clone(),
                containing_symbol_id: identifier.containing_symbol_id.clone(),
                start_line: exact.then_some(identifier.start_line),
                start_column: exact.then_some(identifier.start_column),
                end_line: exact.then_some(identifier.end_line),
                end_column: exact.then_some(identifier.end_column),
                start_byte: exact.then_some(identifier.start_byte),
                end_byte: exact.then_some(identifier.end_byte),
                is_exact: exact,
                provenance: identifier.site_provenance.as_str().to_string(),
                level: if l1_claims.contains(identifier.reference_site_id.as_str()) {
                    1
                } else {
                    2
                },
            },
        );
    }

    for relationship in &file.relationships {
        let exact = relationship.site_is_exact;
        insert_first_site(
            &mut sites,
            &mut indexes,
            StoreReferenceSite {
                reference_site_id: relationship.reference_site_id.clone(),
                path: file.path.clone(),
                language: file.language.clone(),
                containing_symbol_id: Some(relationship.from_symbol_id.clone()),
                start_line: exact.then_some(relationship.start_line).flatten(),
                start_column: exact.then_some(relationship.start_column).flatten(),
                end_line: exact.then_some(relationship.end_line).flatten(),
                end_column: exact.then_some(relationship.end_column).flatten(),
                start_byte: exact.then_some(relationship.start_byte).flatten(),
                end_byte: exact.then_some(relationship.end_byte).flatten(),
                is_exact: exact,
                provenance: relationship.site_provenance.as_str().to_string(),
                level: 1,
            },
        );
    }

    for pending in &file.pending_relationships {
        let exact = pending.site_is_exact;
        insert_first_site(
            &mut sites,
            &mut indexes,
            StoreReferenceSite {
                reference_site_id: pending.reference_site_id.clone(),
                path: file.path.clone(),
                language: file.language.clone(),
                containing_symbol_id: pending
                    .caller_scope_symbol_id
                    .clone()
                    .or_else(|| Some(pending.from_symbol_id.clone())),
                start_line: exact.then_some(pending.start_line),
                start_column: exact.then_some(pending.start_column).flatten(),
                end_line: exact.then_some(pending.end_line).flatten(),
                end_column: exact.then_some(pending.end_column).flatten(),
                start_byte: exact.then_some(pending.start_byte).flatten(),
                end_byte: exact.then_some(pending.end_byte).flatten(),
                is_exact: exact,
                provenance: pending.site_provenance.as_str().to_string(),
                level: 1,
            },
        );
    }

    let mut l1 = Vec::new();
    let mut l2 = Vec::new();
    for site in sites {
        if site.level == 1 {
            l1.push(site);
        } else {
            l2.push(site);
        }
    }
    (l1, l2)
}

fn insert_first_site(
    sites: &mut Vec<StoreReferenceSite>,
    indexes: &mut HashMap<String, usize>,
    site: StoreReferenceSite,
) {
    if let Some(index) = indexes.get(&site.reference_site_id).copied() {
        if site.level == 1 {
            sites[index].level = 1;
        }
        return;
    }
    indexes.insert(site.reference_site_id.clone(), sites.len());
    sites.push(site);
}
