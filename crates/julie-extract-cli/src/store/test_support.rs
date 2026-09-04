use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{RevisionInput, WriteMode, WriteOperation};
use julie_extract_artifact::writer::ArtifactWriter;
use julie_extractors::ExtractionLevel;

use crate::capability_snapshot::artifact_capability_snapshot;
use crate::discovery::{DiscoveryPolicy, FileSelection};
use crate::extraction::extract_artifact_file;

const ORACLE_TIME: &str = "2026-08-08T00:00:00Z";

pub fn write_all_language_fixture(root: &Path) -> Result<Vec<String>, String> {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/extraction");
    let mut copied = BTreeSet::new();
    for language in julie_extractors::supported_languages() {
        let basic = source_root.join(language).join("basic");
        let source = fs::read_dir(&basic)
            .map_err(|error| format!("read {language} fixture: {error}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("source."))
            })
            .or_else(|| {
                let path = basic.join(language);
                path.is_file().then_some(path)
            })
            .ok_or_else(|| format!("{language} fixture has no source file"))?;
        let destination = root.join("languages").join(language);
        fs::create_dir_all(&destination)
            .map_err(|error| format!("create {language} fixture: {error}"))?;
        let file_name = source
            .file_name()
            .ok_or_else(|| format!("{language} fixture source has no name"))?;
        fs::copy(&source, destination.join(file_name))
            .map_err(|error| format!("copy {language} fixture: {error}"))?;
        copied.insert(language.to_string());
    }
    Ok(copied.into_iter().collect())
}

pub fn write_v3_extraction_oracle(root: &Path, database: &Path) -> Result<(), String> {
    let policy =
        DiscoveryPolicy::build(root, database, &[]).map_err(|error| format!("{error:?}"))?;
    let discovered = policy.discover_with_progress(None);
    if !discovered.errors.is_empty() || !discovered.slow_file_skips.is_empty() {
        return Err(format!(
            "oracle discovery failed: {:?} {:?}",
            discovered.errors, discovered.slow_file_skips
        ));
    }
    let mut files = Vec::with_capacity(discovered.supported_files.len());
    for target in discovered.supported_files {
        let FileSelection::Supported { language } = policy.select_file(&target) else {
            return Err(format!(
                "discovered target became unsupported: {}",
                target.root_relative_path
            ));
        };
        files.push(
            extract_artifact_file(
                root,
                &target,
                language,
                ORACLE_TIME.to_string(),
                ExtractionLevel::Full,
            )
            .map_err(|error| format!("oracle extraction failed: {error:?}"))?,
        );
    }
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));

    let root_path = root.to_string_lossy().into_owned();
    let metadata = ArtifactMetadata {
        artifact_id: "task-9-v3-oracle".to_string(),
        root_path: root_path.clone(),
        binary_version: env!("CARGO_PKG_VERSION").to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "task-9-oracle".to_string(),
        capability_snapshot_fingerprint: "task-9-oracle".to_string(),
        created_at: ORACLE_TIME.to_string(),
        updated_at: ORACLE_TIME.to_string(),
    };
    let mut writer =
        ArtifactWriter::open_path(database, metadata).map_err(|error| error.to_string())?;
    writer.stage_capability_snapshot(artifact_capability_snapshot());
    writer
        .write_scan(
            RevisionInput {
                operation: WriteOperation::Scan,
                mode: Some(WriteMode::Force),
                started_at: ORACLE_TIME.to_string(),
                completed_at: ORACLE_TIME.to_string(),
                binary_version: env!("CARGO_PKG_VERSION").to_string(),
                input_root: Some(root_path),
            },
            &files,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}
