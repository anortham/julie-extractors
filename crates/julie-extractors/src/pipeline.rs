use std::path::{Component, Path, PathBuf};
use tree_sitter::{Node, Parser, Tree};

use crate::ExtractionResults;
use crate::base::ExtractionLevel;
use crate::base::RecordOffset;
use crate::base::{NormalizedSpan, ParseDiagnostic, ParseDiagnosticKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Canonicalize once at the outer boundary when given an absolute or external path,
/// strip Windows verbatim prefix (`\\?\`), convert to `/` separator, and pass relative form down.
/// If the path is already a root-relative internal path, avoid all filesystem access.
pub(crate) fn normalize_pipeline_path(file_path: &str, workspace_root: &Path) -> String {
    let path = Path::new(file_path);
    let is_external = path.components().any(|c| matches!(c, Component::ParentDir));

    if !path.is_absolute() && !is_external {
        let normalized = file_path.replace('\\', "/");
        if let Some(stripped) = normalized.strip_prefix("./") {
            return stripped.to_string();
        }
        return normalized;
    }

    let path_to_canonicalize = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(file_path)
    };

    let canonical_path = path_to_canonicalize
        .canonicalize()
        .unwrap_or_else(|_| path_to_canonicalize.clone());
    let canonical_path = strip_verbatim_prefix(&canonical_path);

    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let canonical_root = strip_verbatim_prefix(&canonical_root);

    match crate::utils::paths::to_relative_unix_style(&canonical_path, &canonical_root) {
        Ok(relative) => relative,
        Err(_) => canonical_path.to_string_lossy().replace('\\', "/"),
    }
}

pub(crate) fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

pub fn extract_canonical(
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    extract_canonical_at(file_path, content, workspace_root, ExtractionLevel::Full)
}

pub fn extract_canonical_at(
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let normalized_path = normalize_pipeline_path(file_path, workspace_root);
    if normalized_path.ends_with(".jsonl") {
        return extract_jsonl_canonical(&normalized_path, content, workspace_root, level);
    }

    let (language, pre_parsed_tree) =
        crate::language::detect_language_with_tree(Path::new(&normalized_path), content)
            .ok_or_else(|| anyhow::anyhow!("Unsupported file extension for path: {}", file_path))?;

    if let Some(tree) = pre_parsed_tree {
        extract_canonical_with_tree(
            language,
            tree,
            &normalized_path,
            content,
            workspace_root,
            level,
        )
    } else {
        extract_canonical_with_parse_and_language(
            language,
            &normalized_path,
            content,
            workspace_root,
            level,
            parse_for_language,
        )
    }
}

pub fn extract_canonical_for_language_at(
    language: &str,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let normalized_path = normalize_pipeline_path(file_path, workspace_root);
    if normalized_path.ends_with(".jsonl") {
        return extract_jsonl_canonical(&normalized_path, content, workspace_root, level);
    }

    extract_canonical_with_parse_and_language(
        language,
        &normalized_path,
        content,
        workspace_root,
        level,
        parse_for_language,
    )
}

#[cfg(test)]
pub(crate) fn extract_canonical_with_parse<F>(
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
    parse: F,
) -> Result<ExtractionResults, anyhow::Error>
where
    F: FnOnce(&str, &str, &str) -> Result<Option<Tree>, anyhow::Error>,
{
    let normalized_path = normalize_pipeline_path(file_path, workspace_root);
    let language = crate::language::detect_language_for_source(&normalized_path, content)
        .ok_or_else(|| anyhow::anyhow!("Unsupported file extension for path: {}", file_path))?;
    extract_canonical_with_parse_and_language(
        language,
        &normalized_path,
        content,
        workspace_root,
        level,
        parse,
    )
}

pub(crate) fn extract_canonical_with_tree(
    language: &str,
    tree: Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut results = crate::registry::extract_for_language_at(
        language,
        &tree,
        file_path,
        content,
        workspace_root,
        level,
    )?;
    results.parse_diagnostics = with_tree_diagnostics(&tree, results.parse_diagnostics);
    Ok(results)
}

pub(crate) fn extract_canonical_with_parse_and_language<F>(
    language: &str,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
    parse: F,
) -> Result<ExtractionResults, anyhow::Error>
where
    F: FnOnce(&str, &str, &str) -> Result<Option<Tree>, anyhow::Error>,
{
    let Some(tree) = parse(language, file_path, content)? else {
        return Ok(degraded_parse_failure_result(content));
    };

    extract_canonical_with_tree(language, tree, file_path, content, workspace_root, level)
}

fn extract_jsonl_canonical(
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let normalized_path = normalize_pipeline_path(file_path, workspace_root);
    extract_jsonl_canonical_with_parser_factory(
        &normalized_path,
        content,
        workspace_root,
        level,
        || configured_parser_for_language("json"),
    )
}

pub(crate) fn extract_jsonl_canonical_with_parser_factory<F>(
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
    parser_factory: F,
) -> Result<ExtractionResults, anyhow::Error>
where
    F: FnOnce() -> Result<Parser, anyhow::Error>,
{
    let mut results = ExtractionResults::empty();
    let mut parser = parser_factory()?;

    for (line_delta, byte_delta, line) in jsonl_records(content) {
        let Some(tree) = parse_with_parser(&mut parser, file_path, line)? else {
            let mut record_results = degraded_parse_failure_result(line);
            record_results.apply_record_offset(RecordOffset {
                line_delta,
                byte_delta,
            });
            results.extend(record_results);
            continue;
        };
        let mut record_results = crate::registry::extract_for_language_at(
            "json",
            &tree,
            file_path,
            line,
            workspace_root,
            level,
        )?;
        record_results.parse_diagnostics =
            with_tree_diagnostics(&tree, record_results.parse_diagnostics);
        record_results.apply_record_offset(RecordOffset {
            line_delta,
            byte_delta,
        });
        record_results.rekey_normalized_locations();
        results.extend(record_results);
    }

    Ok(results)
}

fn jsonl_records(content: &str) -> Vec<(u32, u32, &str)> {
    let mut records = Vec::new();
    let mut byte_offset = 0u32;

    for (line_offset, chunk) in content.split_inclusive('\n').enumerate() {
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let line = line.strip_suffix('\r').unwrap_or(line);

        if !line.trim().is_empty() {
            records.push((line_offset as u32, byte_offset, line));
        }

        byte_offset += chunk.len() as u32;
    }

    if !content.ends_with('\n') && !content.is_empty() {
        return records;
    }

    records
}

#[cfg(test)]
thread_local! {
    static PARSE_FOR_LANGUAGE_CALL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn parse_for_language_call_count() -> usize {
    PARSE_FOR_LANGUAGE_CALL_COUNT.with(|c| c.get())
}

#[cfg(test)]
pub(crate) fn reset_parse_for_language_call_count() {
    PARSE_FOR_LANGUAGE_CALL_COUNT.with(|c| c.set(0));
}

pub(crate) fn parse_for_language(
    language: &str,
    file_path: &str,
    content: &str,
) -> Result<Option<Tree>, anyhow::Error> {
    #[cfg(test)]
    PARSE_FOR_LANGUAGE_CALL_COUNT.with(|c| c.set(c.get() + 1));

    let mut parser = configured_parser_for_language_at(language, file_path)?;
    parse_with_parser(&mut parser, file_path, content)
}

pub(crate) fn configured_parser_for_language(language: &str) -> Result<Parser, anyhow::Error> {
    let mut parser = Parser::new();
    let tree_sitter_language = crate::language::get_tree_sitter_language(language)?;
    parser
        .set_language(&tree_sitter_language)
        .map_err(|e| anyhow::anyhow!("Failed to set parser language for {}: {}", language, e))?;

    Ok(parser)
}

pub(crate) fn configured_parser_for_language_at(
    language: &str,
    file_path: &str,
) -> Result<Parser, anyhow::Error> {
    let mut parser = Parser::new();
    let tree_sitter_language =
        crate::language_spec::get_tree_sitter_language_for_path(language, Path::new(file_path))?;
    parser
        .set_language(&tree_sitter_language)
        .map_err(|e| anyhow::anyhow!("Failed to set parser language for {}: {}", language, e))?;

    Ok(parser)
}

fn parse_with_parser(
    parser: &mut Parser,
    _file_path: &str,
    content: &str,
) -> Result<Option<Tree>, anyhow::Error> {
    Ok(parser.parse(content, None))
}

fn degraded_parse_failure_result(content: &str) -> ExtractionResults {
    let mut results = ExtractionResults::empty();
    results
        .parse_diagnostics
        .push(total_parse_failure_diagnostic(content));
    results
}

fn total_parse_failure_diagnostic(content: &str) -> ParseDiagnostic {
    let (end_line, end_column) = content_end_position(content);
    ParseDiagnostic {
        kind: ParseDiagnosticKind::Error,
        message: None,
        start_line: 1,
        start_column: 0,
        end_line,
        end_column,
        start_byte: 0,
        end_byte: content.len() as u32,
    }
}

fn content_end_position(content: &str) -> (u32, u32) {
    let mut line = 1;
    let mut column = 0;

    for byte in content.bytes() {
        if byte == b'\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }

    (line, column)
}

/// The tree's own error and missing spans, followed by whatever the extractor
/// reported for itself. An extractor sees failures the tree cannot express — an
/// error-recovery pass that gave up before it ran out of work leaves no node
/// behind — so its diagnostics are kept rather than overwritten.
fn with_tree_diagnostics(tree: &Tree, extractor: Vec<ParseDiagnostic>) -> Vec<ParseDiagnostic> {
    let mut diagnostics = parse_diagnostics_for_tree(tree);
    diagnostics.extend(extractor);
    diagnostics
}

pub fn parse_diagnostics_for_tree(tree: &Tree) -> Vec<ParseDiagnostic> {
    let mut diagnostics = Vec::new();
    collect_parse_diagnostics(tree.root_node(), &mut diagnostics, 0);
    diagnostics
}

fn collect_parse_diagnostics(node: Node<'_>, diagnostics: &mut Vec<ParseDiagnostic>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.is_error() {
        diagnostics.push(parse_diagnostic_for_node(node, ParseDiagnosticKind::Error));
    }
    if node.is_missing() {
        diagnostics.push(parse_diagnostic_for_node(
            node,
            ParseDiagnosticKind::Missing,
        ));
    }

    if !node.has_error() {
        return;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_parse_diagnostics(child, diagnostics, child_depth);
    }
}

fn parse_diagnostic_for_node(node: Node<'_>, kind: ParseDiagnosticKind) -> ParseDiagnostic {
    let span = NormalizedSpan::from_node(&node);
    ParseDiagnostic {
        kind,
        message: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
    }
}

#[cfg(test)]
#[cfg_attr(
    not(any(feature = "test-golden", feature = "test-certification")),
    allow(dead_code)
)]
pub(crate) fn detect_language_for_path(file_path: &str) -> Result<&'static str, anyhow::Error> {
    let extension = Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    crate::language::detect_language_for_path(Path::new(file_path), "")
        .ok_or_else(|| anyhow::anyhow!("Unsupported file extension: {}", extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_canonical_normalizes_absolute_path() {
        let temp_dir = std::env::temp_dir().join("julie_test_pipeline_norm");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let subdir = temp_dir.join("src");
        std::fs::create_dir_all(&subdir).unwrap();
        let file_path = subdir.join("test.rs");
        let content = "fn test() {}";
        std::fs::write(&file_path, content).unwrap();

        let result = extract_canonical(&file_path.to_string_lossy(), content, &temp_dir)
            .expect("extraction should succeed");

        assert_eq!(result.symbols[0].file_path, "src/test.rs");

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_normalize_pipeline_path_relative_no_canonicalize() {
        let root = Path::new("/definitely/nonexistent/root");
        assert_eq!(normalize_pipeline_path("src/lib.rs", root), "src/lib.rs");
        assert_eq!(normalize_pipeline_path(r"src\lib.rs", root), "src/lib.rs");
        assert_eq!(normalize_pipeline_path("./src/lib.rs", root), "src/lib.rs");
    }

    #[test]
    fn test_strip_verbatim_prefix() {
        let path = Path::new(r"\\?\C:\repo\src\main.rs");
        assert_eq!(
            strip_verbatim_prefix(path),
            PathBuf::from(r"C:\repo\src\main.rs")
        );

        let unc_path = Path::new(r"\\?\UNC\server\share\file.rs");
        assert_eq!(
            strip_verbatim_prefix(unc_path),
            PathBuf::from(r"\\server\share\file.rs")
        );

        let normal_path = Path::new("/var/repo/file.rs");
        assert_eq!(
            strip_verbatim_prefix(normal_path),
            PathBuf::from("/var/repo/file.rs")
        );
    }
}
