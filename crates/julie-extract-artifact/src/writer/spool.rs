//! On-disk spool of extracted files.
//!
//! The spool is a private, single-process staging area: one scan creates it,
//! writes every extracted file to it, replays it through the import passes, and
//! deletes it. Nothing outside this process reads it, so the encoding is free to
//! be compact.
//!
//! Each file is stored as two length-prefixed [postcard] frames:
//!
//! ```text
//! [u32 header_len][header frame][u32 body_len][body frame]
//! ```
//!
//! The header carries everything the planning pass and the file/symbol insert
//! pass need — file row, symbols, parse diagnostics, and the symbol ids the
//! file's child rows reference. Those two passes therefore read headers and seek
//! past the body bytes entirely; only the child-row pass pays to decode a body.
//! Inside a body frame the repeated hash-id, name, and kind strings of the
//! reference-site row families are replaced by indexes into a frame-local string
//! table.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactLiteral,
    ArtifactParseDiagnostic, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus,
    ReferenceSiteProvenance,
};

use super::rows::collect_requested_symbol_ids;

pub type ArtifactSpoolResult<T> = Result<T, ArtifactSpoolError>;

#[derive(Debug)]
pub enum ArtifactSpoolError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Codec {
        path: PathBuf,
        record: Option<usize>,
        message: String,
    },
    Unfinished {
        path: PathBuf,
    },
}

impl std::fmt::Display for ArtifactSpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactSpoolError::Io { path, source } => {
                write!(
                    f,
                    "artifact file spool I/O failed at {}: {source}",
                    path.display()
                )
            }
            ArtifactSpoolError::Codec {
                path,
                record: Some(record),
                message,
            } => write!(
                f,
                "artifact file spool decode failed at {}:{record}: {message}",
                path.display()
            ),
            ArtifactSpoolError::Codec {
                path,
                record: None,
                message,
            } => write!(
                f,
                "artifact file spool encode failed at {}: {message}",
                path.display()
            ),
            ArtifactSpoolError::Unfinished { path } => write!(
                f,
                "artifact file spool must be finished before reading: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ArtifactSpoolError {}

/// Everything the planning pass and the file/symbol insert pass need about one
/// spooled file, so neither has to decode the file's child rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpoolFileHeader {
    pub file_id: String,
    pub path: String,
    pub language: String,
    pub content_hash: String,
    pub content_bytes: i64,
    pub line_count: Option<i64>,
    pub indexed_at: String,
    pub status: FileStatus,
    pub metadata_json: Option<String>,
    pub symbols: Vec<ArtifactSymbol>,
    pub parse_diagnostics: Vec<ArtifactParseDiagnostic>,
    /// Every symbol id this file's rows reference, computed when the file was
    /// spooled. Sorted and deduplicated so a spool encodes reproducibly.
    pub requested_symbol_ids: Vec<String>,
}

impl SpoolFileHeader {
    fn from_file(file: &ArtifactFile) -> Self {
        let mut requested = std::collections::HashSet::new();
        collect_requested_symbol_ids(file, &mut requested);
        let mut requested_symbol_ids = requested.into_iter().collect::<Vec<_>>();
        requested_symbol_ids.sort_unstable();

        Self {
            file_id: file.file_id.clone(),
            path: file.path.clone(),
            language: file.language.clone(),
            content_hash: file.content_hash.clone(),
            content_bytes: file.content_bytes,
            line_count: file.line_count,
            indexed_at: file.indexed_at.clone(),
            status: file.status,
            metadata_json: file.metadata_json.clone(),
            symbols: file.symbols.clone(),
            parse_diagnostics: file.parse_diagnostics.clone(),
            requested_symbol_ids,
        }
    }

    /// The file with empty child-row vectors. The planning and file/symbol insert
    /// passes touch only the columns a header already carries, so they can run
    /// against this instead of paying to decode a body frame.
    pub fn into_file_without_child_rows(self) -> ArtifactFile {
        ArtifactFile {
            file_id: self.file_id,
            path: self.path,
            language: self.language,
            content_hash: self.content_hash,
            content_bytes: self.content_bytes,
            line_count: self.line_count,
            indexed_at: self.indexed_at,
            status: self.status,
            metadata_json: self.metadata_json,
            symbols: self.symbols,
            parse_diagnostics: self.parse_diagnostics,
            symbol_annotations: Vec::new(),
            identifiers: Vec::new(),
            relationships: Vec::new(),
            pending_relationships: Vec::new(),
            type_facts: Vec::new(),
            type_argument_usages: Vec::new(),
            type_arguments: Vec::new(),
            literals: Vec::new(),
            source_regions: Vec::new(),
            structural_facts: Vec::new(),
            complexity_metrics: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SpoolFileBody {
    strings: Vec<String>,
    symbol_annotations: Vec<ArtifactSymbolAnnotation>,
    identifiers: Vec<SpoolIdentifier>,
    relationships: Vec<SpoolRelationship>,
    pending_relationships: Vec<SpoolPendingRelationship>,
    type_facts: Vec<ArtifactTypeFact>,
    type_argument_usages: Vec<ArtifactTypeArgumentUsage>,
    type_arguments: Vec<ArtifactTypeArgument>,
    literals: Vec<ArtifactLiteral>,
    source_regions: Vec<ArtifactSourceRegion>,
    structural_facts: Vec<ArtifactStructuralFact>,
    complexity_metrics: Vec<ArtifactComplexityMetric>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpoolIdentifier {
    identifier_id: String,
    reference_site_id: String,
    name: u32,
    kind: u32,
    containing_symbol_id: Option<u32>,
    target_symbol_id: Option<u32>,
    start_line: i64,
    start_column: i64,
    end_line: i64,
    end_column: i64,
    start_byte: i64,
    end_byte: i64,
    site_is_exact: bool,
    site_provenance: ReferenceSiteProvenance,
    confidence: f64,
    code_context: Option<String>,
    metadata_json: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpoolRelationship {
    relationship_id: String,
    reference_site_id: String,
    from_symbol_id: u32,
    to_symbol_id: u32,
    kind: u32,
    start_line: Option<i64>,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    site_is_exact: bool,
    site_provenance: ReferenceSiteProvenance,
    confidence: f64,
    metadata_json: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpoolPendingRelationship {
    pending_relationship_id: String,
    reference_site_id: String,
    from_symbol_id: u32,
    caller_scope_symbol_id: Option<u32>,
    kind: u32,
    target_display_name: u32,
    target_terminal_name: u32,
    target_receiver: Option<u32>,
    target_namespace_json: u32,
    target_import_context: Option<u32>,
    start_line: i64,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    site_is_exact: bool,
    site_provenance: ReferenceSiteProvenance,
    confidence: f64,
    metadata_json: Option<String>,
}

#[derive(Default)]
struct Interner {
    table: Vec<String>,
    index: HashMap<String, u32>,
}

impl Interner {
    fn intern(&mut self, value: &str) -> u32 {
        if let Some(existing) = self.index.get(value) {
            return *existing;
        }
        let slot = self.table.len() as u32;
        self.table.push(value.to_string());
        self.index.insert(value.to_string(), slot);
        slot
    }

    fn intern_optional(&mut self, value: Option<&String>) -> Option<u32> {
        value.map(|value| self.intern(value))
    }
}

struct StringTable<'a> {
    strings: &'a [String],
    path: &'a Path,
    record: usize,
}

impl StringTable<'_> {
    fn get(&self, slot: u32) -> ArtifactSpoolResult<String> {
        self.strings
            .get(slot as usize)
            .cloned()
            .ok_or_else(|| ArtifactSpoolError::Codec {
                path: self.path.to_path_buf(),
                record: Some(self.record),
                message: format!(
                    "string slot {slot} is outside the frame table of {} entries",
                    self.strings.len()
                ),
            })
    }

    fn get_optional(&self, slot: Option<u32>) -> ArtifactSpoolResult<Option<String>> {
        slot.map(|slot| self.get(slot)).transpose()
    }
}

pub struct ArtifactFileSpool {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    len: usize,
}

impl ArtifactFileSpool {
    pub fn create(path: impl AsRef<Path>) -> ArtifactSpoolResult<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::create(&path).map_err(|source| ArtifactSpoolError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(Self {
            path,
            writer: Some(BufWriter::new(file)),
            len: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, file: &ArtifactFile) -> ArtifactSpoolResult<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Err(ArtifactSpoolError::Unfinished {
                path: self.path.clone(),
            });
        };

        let header = postcard::to_stdvec(&SpoolFileHeader::from_file(file))
            .map_err(|source| encode_error(&self.path, source))?;
        let body = postcard::to_stdvec(&encode_body(file))
            .map_err(|source| encode_error(&self.path, source))?;

        for frame in [header.as_slice(), body.as_slice()] {
            let length = u32::try_from(frame.len()).map_err(|_| ArtifactSpoolError::Codec {
                path: self.path.clone(),
                record: None,
                message: format!("frame of {} bytes exceeds the 4 GiB limit", frame.len()),
            })?;
            writer
                .write_all(&length.to_le_bytes())
                .and_then(|()| writer.write_all(frame))
                .map_err(|source| ArtifactSpoolError::Io {
                    path: self.path.clone(),
                    source,
                })?;
        }

        self.len += 1;
        Ok(())
    }

    pub fn finish(&mut self) -> ArtifactSpoolResult<()> {
        let Some(mut writer) = self.writer.take() else {
            return Ok(());
        };
        writer.flush().map_err(|source| ArtifactSpoolError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// A cursor that decodes headers and skips or decodes bodies on demand.
    pub fn reader(&self) -> ArtifactSpoolResult<ArtifactFileSpoolReader> {
        if self.writer.is_some() {
            return Err(ArtifactSpoolError::Unfinished {
                path: self.path.clone(),
            });
        }
        let file = File::open(&self.path).map_err(|source| ArtifactSpoolError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(ArtifactFileSpoolReader {
            path: self.path.clone(),
            reader: BufReader::new(file),
            record: 0,
            pending_body: None,
            frame: Vec::new(),
        })
    }

    /// Every spooled file, child rows included. Callers that only need file-level
    /// facts should use [`ArtifactFileSpool::reader`] instead.
    pub fn iter(&self) -> ArtifactSpoolResult<ArtifactFileSpoolIter> {
        Ok(ArtifactFileSpoolIter {
            reader: self.reader()?,
        })
    }
}

pub struct ArtifactFileSpoolReader {
    path: PathBuf,
    reader: BufReader<File>,
    record: usize,
    pending_body: Option<u32>,
    frame: Vec<u8>,
}

impl ArtifactFileSpoolReader {
    /// The next file header. Any body left unread by the previous call is skipped.
    pub fn next_header(&mut self) -> Option<ArtifactSpoolResult<SpoolFileHeader>> {
        if let Err(error) = self.skip_pending_body() {
            return Some(Err(error));
        }
        let length = match self.read_frame_length() {
            Ok(Some(length)) => length,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        self.record += 1;
        Some(self.read_header_frame(length))
    }

    fn read_header_frame(&mut self, length: u32) -> ArtifactSpoolResult<SpoolFileHeader> {
        let record = self.record;
        let path = self.path.clone();
        let header = {
            let frame = self.read_frame(length)?;
            postcard::from_bytes::<SpoolFileHeader>(frame)
                .map_err(|source| decode_error(&path, record, source))?
        };
        self.pending_body =
            Some(
                self.read_frame_length()?
                    .ok_or_else(|| ArtifactSpoolError::Codec {
                        path,
                        record: Some(record),
                        message: "header frame is not followed by a body frame".to_string(),
                    })?,
            );
        Ok(header)
    }

    /// The full file for the header just returned, decoding its body frame.
    pub fn read_file(&mut self, header: SpoolFileHeader) -> ArtifactSpoolResult<ArtifactFile> {
        let Some(length) = self.pending_body.take() else {
            return Err(ArtifactSpoolError::Codec {
                path: self.path.clone(),
                record: Some(self.record),
                message: "body frame was already consumed".to_string(),
            });
        };
        let record = self.record;
        let path = self.path.clone();
        let frame = self.read_frame(length)?;
        let body = postcard::from_bytes::<SpoolFileBody>(frame)
            .map_err(|source| decode_error(&path, record, source))?;
        decode_body(header, body, &path, record)
    }

    fn skip_pending_body(&mut self) -> ArtifactSpoolResult<()> {
        let Some(length) = self.pending_body.take() else {
            return Ok(());
        };
        self.reader
            .seek_relative(i64::from(length))
            .map_err(|source| ArtifactSpoolError::Io {
                path: self.path.clone(),
                source,
            })
    }

    fn read_frame_length(&mut self) -> ArtifactSpoolResult<Option<u32>> {
        let mut length = [0u8; 4];
        match self.reader.read_exact(&mut length) {
            Ok(()) => Ok(Some(u32::from_le_bytes(length))),
            Err(source) if source.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
            Err(source) => Err(ArtifactSpoolError::Io {
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn read_frame(&mut self, length: u32) -> ArtifactSpoolResult<&[u8]> {
        self.frame.clear();
        self.frame.resize(length as usize, 0);
        self.reader
            .read_exact(&mut self.frame)
            .map_err(|source| ArtifactSpoolError::Io {
                path: self.path.clone(),
                source,
            })?;
        Ok(&self.frame)
    }
}

pub struct ArtifactFileSpoolIter {
    reader: ArtifactFileSpoolReader,
}

impl Iterator for ArtifactFileSpoolIter {
    type Item = ArtifactSpoolResult<ArtifactFile>;

    fn next(&mut self) -> Option<Self::Item> {
        let header = match self.reader.next_header()? {
            Ok(header) => header,
            Err(error) => return Some(Err(error)),
        };
        Some(self.reader.read_file(header))
    }
}

fn encode_body(file: &ArtifactFile) -> SpoolFileBody {
    let mut interner = Interner::default();

    let identifiers = file
        .identifiers
        .iter()
        .map(|row| SpoolIdentifier {
            identifier_id: row.identifier_id.clone(),
            reference_site_id: row.reference_site_id.clone(),
            name: interner.intern(&row.name),
            kind: interner.intern(&row.kind),
            containing_symbol_id: interner.intern_optional(row.containing_symbol_id.as_ref()),
            target_symbol_id: interner.intern_optional(row.target_symbol_id.as_ref()),
            start_line: row.start_line,
            start_column: row.start_column,
            end_line: row.end_line,
            end_column: row.end_column,
            start_byte: row.start_byte,
            end_byte: row.end_byte,
            site_is_exact: row.site_is_exact,
            site_provenance: row.site_provenance,
            confidence: row.confidence,
            code_context: row.code_context.clone(),
            metadata_json: row.metadata_json.clone(),
        })
        .collect();

    let relationships = file
        .relationships
        .iter()
        .map(|row| SpoolRelationship {
            relationship_id: row.relationship_id.clone(),
            reference_site_id: row.reference_site_id.clone(),
            from_symbol_id: interner.intern(&row.from_symbol_id),
            to_symbol_id: interner.intern(&row.to_symbol_id),
            kind: interner.intern(&row.kind),
            start_line: row.start_line,
            start_column: row.start_column,
            end_line: row.end_line,
            end_column: row.end_column,
            start_byte: row.start_byte,
            end_byte: row.end_byte,
            site_is_exact: row.site_is_exact,
            site_provenance: row.site_provenance,
            confidence: row.confidence,
            metadata_json: row.metadata_json.clone(),
        })
        .collect();

    let pending_relationships = file
        .pending_relationships
        .iter()
        .map(|row| SpoolPendingRelationship {
            pending_relationship_id: row.pending_relationship_id.clone(),
            reference_site_id: row.reference_site_id.clone(),
            from_symbol_id: interner.intern(&row.from_symbol_id),
            caller_scope_symbol_id: interner.intern_optional(row.caller_scope_symbol_id.as_ref()),
            kind: interner.intern(&row.kind),
            target_display_name: interner.intern(&row.target_display_name),
            target_terminal_name: interner.intern(&row.target_terminal_name),
            target_receiver: interner.intern_optional(row.target_receiver.as_ref()),
            target_namespace_json: interner.intern(&row.target_namespace_json),
            target_import_context: interner.intern_optional(row.target_import_context.as_ref()),
            start_line: row.start_line,
            start_column: row.start_column,
            end_line: row.end_line,
            end_column: row.end_column,
            start_byte: row.start_byte,
            end_byte: row.end_byte,
            site_is_exact: row.site_is_exact,
            site_provenance: row.site_provenance,
            confidence: row.confidence,
            metadata_json: row.metadata_json.clone(),
        })
        .collect();

    SpoolFileBody {
        strings: interner.table,
        symbol_annotations: file.symbol_annotations.clone(),
        identifiers,
        relationships,
        pending_relationships,
        type_facts: file.type_facts.clone(),
        type_argument_usages: file.type_argument_usages.clone(),
        type_arguments: file.type_arguments.clone(),
        literals: file.literals.clone(),
        source_regions: file.source_regions.clone(),
        structural_facts: file.structural_facts.clone(),
        complexity_metrics: file.complexity_metrics.clone(),
    }
}

fn decode_body(
    header: SpoolFileHeader,
    body: SpoolFileBody,
    path: &Path,
    record: usize,
) -> ArtifactSpoolResult<ArtifactFile> {
    let strings = StringTable {
        strings: &body.strings,
        path,
        record,
    };

    let identifiers = body
        .identifiers
        .into_iter()
        .map(|row| {
            Ok(ArtifactIdentifier {
                identifier_id: row.identifier_id,
                reference_site_id: row.reference_site_id,
                name: strings.get(row.name)?,
                kind: strings.get(row.kind)?,
                containing_symbol_id: strings.get_optional(row.containing_symbol_id)?,
                target_symbol_id: strings.get_optional(row.target_symbol_id)?,
                start_line: row.start_line,
                start_column: row.start_column,
                end_line: row.end_line,
                end_column: row.end_column,
                start_byte: row.start_byte,
                end_byte: row.end_byte,
                site_is_exact: row.site_is_exact,
                site_provenance: row.site_provenance,
                confidence: row.confidence,
                code_context: row.code_context,
                metadata_json: row.metadata_json,
            })
        })
        .collect::<ArtifactSpoolResult<Vec<_>>>()?;

    let relationships = body
        .relationships
        .into_iter()
        .map(|row| {
            Ok(ArtifactRelationship {
                relationship_id: row.relationship_id,
                reference_site_id: row.reference_site_id,
                from_symbol_id: strings.get(row.from_symbol_id)?,
                to_symbol_id: strings.get(row.to_symbol_id)?,
                kind: strings.get(row.kind)?,
                start_line: row.start_line,
                start_column: row.start_column,
                end_line: row.end_line,
                end_column: row.end_column,
                start_byte: row.start_byte,
                end_byte: row.end_byte,
                site_is_exact: row.site_is_exact,
                site_provenance: row.site_provenance,
                confidence: row.confidence,
                metadata_json: row.metadata_json,
            })
        })
        .collect::<ArtifactSpoolResult<Vec<_>>>()?;

    let pending_relationships = body
        .pending_relationships
        .into_iter()
        .map(|row| {
            Ok(ArtifactPendingRelationship {
                pending_relationship_id: row.pending_relationship_id,
                reference_site_id: row.reference_site_id,
                from_symbol_id: strings.get(row.from_symbol_id)?,
                caller_scope_symbol_id: strings.get_optional(row.caller_scope_symbol_id)?,
                kind: strings.get(row.kind)?,
                target_display_name: strings.get(row.target_display_name)?,
                target_terminal_name: strings.get(row.target_terminal_name)?,
                target_receiver: strings.get_optional(row.target_receiver)?,
                target_namespace_json: strings.get(row.target_namespace_json)?,
                target_import_context: strings.get_optional(row.target_import_context)?,
                start_line: row.start_line,
                start_column: row.start_column,
                end_line: row.end_line,
                end_column: row.end_column,
                start_byte: row.start_byte,
                end_byte: row.end_byte,
                site_is_exact: row.site_is_exact,
                site_provenance: row.site_provenance,
                confidence: row.confidence,
                metadata_json: row.metadata_json,
            })
        })
        .collect::<ArtifactSpoolResult<Vec<_>>>()?;

    Ok(ArtifactFile {
        file_id: header.file_id,
        path: header.path,
        language: header.language,
        content_hash: header.content_hash,
        content_bytes: header.content_bytes,
        line_count: header.line_count,
        indexed_at: header.indexed_at,
        status: header.status,
        metadata_json: header.metadata_json,
        symbols: header.symbols,
        parse_diagnostics: header.parse_diagnostics,
        symbol_annotations: body.symbol_annotations,
        identifiers,
        relationships,
        pending_relationships,
        type_facts: body.type_facts,
        type_argument_usages: body.type_argument_usages,
        type_arguments: body.type_arguments,
        literals: body.literals,
        source_regions: body.source_regions,
        structural_facts: body.structural_facts,
        complexity_metrics: body.complexity_metrics,
    })
}

fn encode_error(path: &Path, source: postcard::Error) -> ArtifactSpoolError {
    ArtifactSpoolError::Codec {
        path: path.to_path_buf(),
        record: None,
        message: source.to_string(),
    }
}

fn decode_error(path: &Path, record: usize, source: postcard::Error) -> ArtifactSpoolError {
    ArtifactSpoolError::Codec {
        path: path.to_path_buf(),
        record: Some(record),
        message: source.to_string(),
    }
}
