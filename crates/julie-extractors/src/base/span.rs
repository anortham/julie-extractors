use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedSpan {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub start_byte: u32,
    pub end_byte: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordOffset {
    pub line_delta: u32,
    pub byte_delta: u32,
}

impl NormalizedSpan {
    pub fn from_node(node: &Node) -> Self {
        let start_pos = node.start_position();
        let end_pos = node.end_position();

        Self {
            start_line: start_pos.row as u32 + 1,
            start_column: start_pos.column as u32,
            end_line: end_pos.row as u32 + 1,
            end_column: end_pos.column as u32,
            start_byte: node.start_byte() as u32,
            end_byte: node.end_byte() as u32,
        }
    }

    pub fn from_content_range(content: &str, start_byte: usize, end_byte: usize) -> Option<Self> {
        let start_prefix = content.get(..start_byte)?;
        let end_prefix = content.get(..end_byte)?;

        Some(Self {
            start_line: start_prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1,
            start_column: start_prefix
                .rsplit_once('\n')
                .map(|(_, tail)| tail.len())
                .unwrap_or(start_prefix.len()) as u32,
            end_line: end_prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1,
            end_column: end_prefix
                .rsplit_once('\n')
                .map(|(_, tail)| tail.len())
                .unwrap_or(end_prefix.len()) as u32,
            start_byte: start_byte as u32,
            end_byte: end_byte as u32,
        })
    }

    pub fn from_content_range_with_line_starts(
        content: &str,
        line_starts: &[usize],
        start_byte: usize,
        end_byte: usize,
    ) -> Option<Self> {
        content.get(..start_byte)?;
        content.get(..end_byte)?;

        if line_starts.is_empty() {
            return Self::from_content_range(content, start_byte, end_byte);
        }

        let (start_line, start_column) = line_column_for_byte(line_starts, start_byte);
        let (end_line, end_column) = line_column_for_byte(line_starts, end_byte);

        Some(Self {
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte: start_byte as u32,
            end_byte: end_byte as u32,
        })
    }

    pub fn with_offset(self, offset: RecordOffset) -> Self {
        Self {
            start_line: self.start_line + offset.line_delta,
            start_column: self.start_column,
            end_line: self.end_line + offset.line_delta,
            end_column: self.end_column,
            start_byte: self.start_byte + offset.byte_delta,
            end_byte: self.end_byte + offset.byte_delta,
        }
    }
}

fn line_column_for_byte(line_starts: &[usize], byte: usize) -> (u32, u32) {
    let line_index = line_starts
        .partition_point(|line_start| *line_start <= byte)
        .saturating_sub(1);
    (
        line_index as u32 + 1,
        byte.saturating_sub(line_starts[line_index]) as u32,
    )
}

pub fn normalize_file_path(file_path: &str, workspace_root: &Path) -> String {
    let path_to_canonicalize = if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else {
        workspace_root.join(file_path)
    };

    let canonical_path = path_to_canonicalize.canonicalize().unwrap_or_else(|e| {
        warn!(
            "⚠️  Failed to canonicalize path '{}': {} - using joined path",
            path_to_canonicalize.display(),
            e
        );
        path_to_canonicalize.clone()
    });

    match crate::utils::paths::to_relative_unix_style(&canonical_path, workspace_root) {
        Ok(relative_path) => relative_path,
        Err(e) => {
            if canonical_path.is_absolute() {
                warn!(
                    "⚠️  Failed to convert to relative path '{}': {} - using absolute as fallback",
                    canonical_path.display(),
                    e
                );
                canonical_path.to_string_lossy().replace('\\', "/")
            } else {
                canonical_path.to_string_lossy().replace('\\', "/")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_content_range_matches_prefix_scan() {
        let content = "alpha\nβeta\r\ngamma\n";
        let line_starts = [0, 6, 13, 19];

        let expected = NormalizedSpan::from_content_range(content, 8, 18).unwrap();
        let actual =
            NormalizedSpan::from_content_range_with_line_starts(content, &line_starts, 8, 18)
                .unwrap();

        assert_eq!(actual, expected);
    }
}
