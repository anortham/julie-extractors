use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::ParseDiagnostic;

pub(crate) mod diagnostics;

#[derive(Debug)]
pub struct ParsedSource {
    pub language: &'static str,
    pub tree: tree_sitter::Tree,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug)]
pub enum SyntaxError {
    UnsupportedLanguage { path: PathBuf },
    UnsupportedContainer { path: PathBuf },
    InputTooLarge { bytes: usize },
    Cancelled,
    DeadlineExceeded,
    ParseFailed { source: anyhow::Error },
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedLanguage { path } => {
                write!(f, "unsupported language for path: {}", path.display())
            }
            Self::UnsupportedContainer { path } => {
                write!(f, "unsupported container for path: {}", path.display())
            }
            Self::InputTooLarge { bytes } => {
                write!(f, "input too large: {bytes} bytes")
            }
            Self::Cancelled => write!(f, "syntax parsing was cancelled"),
            Self::DeadlineExceeded => write!(f, "syntax parsing deadline exceeded"),
            Self::ParseFailed { source } => write!(f, "syntax parse failed: {source}"),
        }
    }
}

impl std::error::Error for SyntaxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ParseFailed { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

pub struct SyntaxOptions<'a> {
    pub deadline: Option<Instant>,
    pub cancelled: Option<&'a AtomicBool>,
    pub max_source_bytes: usize,
}

impl<'a> Default for SyntaxOptions<'a> {
    fn default() -> Self {
        Self {
            deadline: None,
            cancelled: None,
            max_source_bytes: (u32::MAX - 1) as usize,
        }
    }
}

impl SyntaxOptions<'_> {
    pub(crate) fn check(&self) -> Result<(), SyntaxError> {
        if let Some(cancelled) = self.cancelled {
            if cancelled.load(Ordering::Acquire) {
                return Err(SyntaxError::Cancelled);
            }
        }
        if let Some(deadline) = self.deadline {
            #[cfg(test)]
            let now = TEST_CLOCK.with(|c| c.get().unwrap_or_else(Instant::now));
            #[cfg(not(test))]
            let now = Instant::now();

            if now >= deadline {
                return Err(SyntaxError::DeadlineExceeded);
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_source_len(bytes: usize, configured_max: usize) -> Result<(), SyntaxError> {
    let effective_max = configured_max.min((u32::MAX - 1) as usize);
    if bytes > effective_max {
        return Err(SyntaxError::InputTooLarge { bytes });
    }
    Ok(())
}

pub fn parse_source(file_path: &Path, source: &str) -> Result<ParsedSource, SyntaxError> {
    parse_source_with_options(file_path, source, &SyntaxOptions::default())
}

pub fn parse_source_with_options(
    file_path: &Path,
    source: &str,
    options: &SyntaxOptions<'_>,
) -> Result<ParsedSource, SyntaxError> {
    options.check()?;
    validate_source_len(source.len(), options.max_source_bytes)?;
    let is_jsonl = file_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
        || file_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(".jsonl"));
    if is_jsonl {
        return Err(SyntaxError::UnsupportedContainer {
            path: file_path.to_path_buf(),
        });
    }

    let (language, pre_parsed) =
        crate::language_spec::source_detection::detect_strict(file_path, source, options)?
            .ok_or_else(|| SyntaxError::UnsupportedLanguage {
                path: file_path.to_path_buf(),
            })?;

    let tree = match pre_parsed {
        Some(tree) => tree,
        None => {
            #[cfg(test)]
            SYNTAX_DIRECT_PARSE_COUNT.with(|c| c.set(c.get() + 1));
            let grammar =
                crate::language_spec::get_tree_sitter_language_for_path(language, file_path)
                    .map_err(|source| SyntaxError::ParseFailed { source })?;
            parse_tree_with_options(&grammar, source, options)?
        }
    };

    let diagnostics = diagnostics::collect(&tree, options)?;
    options.check()?;
    Ok(ParsedSource {
        language,
        tree,
        diagnostics,
    })
}

pub(crate) fn parse_tree_with_options(
    grammar: &tree_sitter::Language,
    source: &str,
    options: &SyntaxOptions<'_>,
) -> Result<tree_sitter::Tree, SyntaxError> {
    use std::ops::ControlFlow;
    options.check()?;

    let mut parser = tree_sitter::Parser::new();
    #[cfg(test)]
    if TEST_PARSER_SETUP_FAIL.with(|c| c.get()) {
        return Err(SyntaxError::ParseFailed {
            source: anyhow::anyhow!("custom parser setup failed"),
        });
    }
    parser
        .set_language(grammar)
        .map_err(|e| SyntaxError::ParseFailed { source: e.into() })?;
    options.check()?;

    let mut stopped = None;
    let tree = {
        let mut progress = |_state: &tree_sitter::ParseState| {
            #[cfg(test)]
            TEST_PROGRESS_HOOK.with(|hook| {
                if let Some(hook) = hook.borrow_mut().as_mut() {
                    hook(_state);
                }
            });

            if let Err(error) = options.check() {
                stopped = Some(error);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };

        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut input = |offset: usize, _: tree_sitter::Point| {
            (offset < len).then(|| &bytes[offset..]).unwrap_or_default()
        };

        parser.parse_with_options(
            &mut input,
            None,
            Some(tree_sitter::ParseOptions::new().progress_callback(&mut progress)),
        )
    };

    complete_parsed_tree(tree, stopped)
}

pub(crate) fn complete_parsed_tree(
    tree: Option<tree_sitter::Tree>,
    stopped: Option<SyntaxError>,
) -> Result<tree_sitter::Tree, SyntaxError> {
    if let Some(error) = stopped {
        return Err(error);
    }
    tree.ok_or_else(|| SyntaxError::ParseFailed {
        source: anyhow::anyhow!("parser returned no tree"),
    })
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_CLOCK: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    pub(crate) static TEST_PROGRESS_HOOK: std::cell::RefCell<Option<Box<dyn FnMut(&tree_sitter::ParseState)>>> = const { std::cell::RefCell::new(None) };
    pub(crate) static SYNTAX_DIRECT_PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static TEST_PARSER_SETUP_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn set_test_parser_setup_fail(fail: bool) {
    TEST_PARSER_SETUP_FAIL.with(|c| c.set(fail));
}

#[cfg(test)]
pub(crate) fn set_test_clock(instant: Option<Instant>) {
    TEST_CLOCK.with(|c| c.set(instant));
}

#[cfg(test)]
pub(crate) fn set_test_progress_hook(hook: Option<Box<dyn FnMut(&tree_sitter::ParseState)>>) {
    TEST_PROGRESS_HOOK.with(|h| *h.borrow_mut() = hook);
}

#[cfg(test)]
pub(crate) fn set_test_diagnostic_node_hook(hook: Option<Box<dyn FnMut(&tree_sitter::Node)>>) {
    diagnostics::TEST_DIAGNOSTIC_NODE_HOOK.with(|h| *h.borrow_mut() = hook);
}

#[cfg(test)]
pub(crate) fn syntax_direct_parse_count() -> usize {
    SYNTAX_DIRECT_PARSE_COUNT.with(|c| c.get())
}

#[cfg(test)]
pub(crate) fn reset_syntax_direct_parse_count() {
    SYNTAX_DIRECT_PARSE_COUNT.with(|c| c.set(0));
}
