//! Bounded parse-error recovery for Erlang.
//!
//! `tree-sitter-erlang` has no preprocessor, so a macro that expands to a partial
//! clause head — `-define(WITH_STACKTRACE(T, R, S), T:R:S ->).` used as a `catch`
//! clause — cannot parse. The damage is not local: the grammar reinterprets the
//! rest of the file as a continuation of the broken construct, so every top-level
//! form after the first such macro is lost. `telemetry` 1.3.0 loses four of its
//! eight exports that way.
//!
//! The cascade is unbounded at file scope but contained at form scope. Recovery
//! therefore re-parses the file from successive top-level form starts: everything
//! before the resume point is blanked to spaces (newlines kept), so the re-parsed
//! buffer has byte-for-byte the same offsets and line numbers as the original and
//! its nodes can be read against the original content. tree-sitter re-synchronises
//! on its own, which is why this needs no Erlang form tokenizer.
//!
//! The budget is deliberately small and the trigger is narrow:
//! - a clean parse does zero extra work ([`recover`] returns before touching the
//!   parser when the root has no error);
//! - at most [`MAX_RECOVERY_PARSES`] re-parses per file;
//! - resume points are only column-0 form starts, never offsets inside a
//!   string, a comment, or a multiline quoted atom, so neither doc-comment prose
//!   nor quoted text is re-read as code.
//!
//! Literal extents come from [`lexical`], not from the parse tree. The tree is
//! the broken artifact recovery exists to work around: an unclosed `"` never
//! becomes a `string` node, so a tree-derived literal map cannot see it and the
//! form-shaped lines in its interior would be re-parsed as declarations.

use tree_sitter::{Node, Parser, Tree};

use super::lexical::LiteralSpans;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Hard cap on re-parses per file. Recovery only runs on a file that already
/// failed to parse, and each pass must advance to a later resume point, so this
/// bounds the worst case rather than describing the common one.
pub(super) const MAX_RECOVERY_PARSES: usize = 32;

/// Node kinds a recovered tree may contribute as a top-level declaration.
/// Anything else it produces — expression fragments the broken region left
/// behind — is ignored.
pub(super) const RECOVERABLE_DECLARATION_KINDS: &[&str] = &[
    "module_attribute",
    "export_attribute",
    "export_type_attribute",
    "compile_options_attribute",
    "behaviour_attribute",
    "import_attribute",
    "record_decl",
    "pp_define",
    "pp_include",
    "pp_include_lib",
    "type_alias",
    "opaque",
    "callback",
    "spec",
    "wild_attribute",
    "fun_decl",
];

/// The outcome of recovering one file: the extra trees, in pass order, the
/// literal map their declarations are filtered against, and the offset recovery
/// gave up at when the budget ran out before the errors did.
#[derive(Default)]
pub(super) struct Recovery {
    pub(super) trees: Vec<Tree>,
    pub(super) literals: LiteralSpans,
    pub(super) exhausted_at: Option<usize>,
}

impl Recovery {
    /// Whether a recovered declaration starting at `offset` is literal text.
    ///
    /// The resume-point filter keeps recovery from cutting into a literal, but a
    /// pass that resumes legally can still leave an unresolved error region whose
    /// re-parse invents declarations further down inside one.
    pub(super) fn is_literal_text(&self, offset: usize) -> bool {
        self.literals.contains_strictly(offset)
    }
}

/// Re-parse `content` from successive form starts until the tail parses clean,
/// no resume point is left, or the budget runs out. An empty tree list means the
/// primary parse needs no recovery.
pub(super) fn recover(content: &str, primary: &Tree) -> Recovery {
    if !primary.root_node().has_error() {
        return Recovery::default();
    }

    let Ok(language) = crate::language::get_tree_sitter_language("erlang") else {
        return Recovery::default();
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Recovery::default();
    }

    let literals = LiteralSpans::scan(content);
    let resume_points = resume_points(content, &literals);
    let mut trees = Vec::new();
    let mut error_start = first_error_start(&primary.root_node(), 0).unwrap_or(0);
    let mut next_index = 0;
    let mut exhausted_at = None;

    for pass in 0..MAX_RECOVERY_PARSES {
        let Some(index) = resume_points
            .iter()
            .position(|offset| *offset > error_start)
            .map(|found| found.max(next_index))
        else {
            break;
        };
        let Some(&cut) = resume_points.get(index) else {
            break;
        };
        let Some(tree) = parser.parse(blank_before(content, cut), None) else {
            break;
        };

        let unresolved = tree.root_node().has_error();
        let next_error_start = first_error_start(&tree.root_node(), 0);
        trees.push(tree);

        if !unresolved {
            break;
        }
        next_index = index + 1;
        error_start = next_error_start.unwrap_or(cut);
        if pass + 1 == MAX_RECOVERY_PARSES {
            exhausted_at = Some(error_start);
        }
    }

    Recovery {
        trees,
        literals,
        exhausted_at,
    }
}

/// A copy of `content` with every byte before `cut` replaced by a space, except
/// newlines. Same byte length, same line breaks, so a node parsed from the copy
/// carries offsets and line/column positions valid against the original.
pub(super) fn blank_before(content: &str, cut: usize) -> String {
    let blanked = content
        .bytes()
        .enumerate()
        .map(|(offset, byte)| {
            if offset >= cut || byte == b'\n' {
                byte
            } else {
                b' '
            }
        })
        .collect::<Vec<u8>>();

    String::from_utf8(blanked).unwrap_or_else(|_| content.to_string())
}

/// Byte offsets of lines that can begin a top-level form: column 0, starting
/// with an attribute `-`, a macro `?`, or an atom. Offsets inside a literal are
/// excluded, so neither prose in a `"""` doc block nor text in a multiline
/// quoted atom is ever used as a resume point.
///
/// Only offsets STRICTLY inside a literal are rejected. A literal that begins
/// at the offset is that form's own head — `'quoted name'(X) -> X.` is a legal
/// Erlang function, and so is every unquoted head, which is an `atom` too.
fn resume_points(content: &str, literals: &LiteralSpans) -> Vec<usize> {
    let mut points = Vec::new();
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        if starts_form(line) && !literals.contains_strictly(offset) {
            points.push(offset);
        }
        offset += line.len();
    }

    points
}

fn starts_form(line: &str) -> bool {
    matches!(
        line.as_bytes().first(),
        Some(b'-') | Some(b'?') | Some(b'\'') | Some(b'a'..=b'z')
    )
}

/// Start offset of the first error below `node`. The node itself is never
/// reported: a whole-file failure parses to a root `ERROR`, and resuming at the
/// start of the file would make no progress.
fn first_error_start(node: &Node, depth: u32) -> Option<usize> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    let child_depth = child_tree_depth(depth)?;

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "ERROR" || child.is_missing() {
            return Some(child.start_byte());
        }
        if child.has_error()
            && let Some(start) = first_error_start(&child, child_depth)
        {
            return Some(start);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(code: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&crate::language::get_tree_sitter_language("erlang").unwrap())
            .unwrap();
        parser.parse(code, None).unwrap()
    }

    #[test]
    fn clean_source_does_no_recovery_parses() {
        let code = "-module(bank).\n-export([open/1]).\nopen(Id) -> Id.\n";
        let tree = parse(code);

        assert!(!tree.root_node().has_error());
        assert!(recover(code, &tree).trees.is_empty());
    }

    #[test]
    fn blanking_preserves_byte_length_and_line_breaks() {
        let code = "-module(bank).\nopen(Id) -> Id.\n";
        let cut = code.find("open").unwrap();
        let blanked = blank_before(code, cut);

        assert_eq!(blanked.len(), code.len());
        assert_eq!(blanked.matches('\n').count(), code.matches('\n').count());
        assert!(blanked[cut..].starts_with("open(Id)"));
        assert!(blanked[..cut].trim().is_empty());
    }

    #[test]
    fn blanking_preserves_offsets_for_multibyte_content() {
        let code = "-module(bank).\n%% ☃ snowman\nopen(Id) -> Id.\n";
        let cut = code.find("open").unwrap();
        let blanked = blank_before(code, cut);

        assert_eq!(blanked.len(), code.len());
        assert_eq!(&blanked[cut..], &code[cut..]);
    }

    #[test]
    fn resume_points_skip_lines_inside_strings_and_comments() {
        let code = "-module(bank).\n-doc \"\"\"\nopen(Id) -> Id.\n\"\"\".\n%% audit(A) -> A.\nreal(X) -> X.\n";
        let points = resume_points(code, &LiteralSpans::scan(code));

        let lines: Vec<&str> = points
            .iter()
            .map(|offset| code[*offset..].lines().next().unwrap_or_default())
            .collect();

        assert!(lines.contains(&"real(X) -> X."), "got {lines:?}");
        assert!(!lines.contains(&"open(Id) -> Id."), "got {lines:?}");
        assert!(
            !lines.iter().any(|line| line.starts_with("%%")),
            "got {lines:?}"
        );
    }

    #[test]
    fn resume_points_skip_lines_inside_a_multiline_quoted_atom() {
        let code = "-module(bank).\nlabel() -> 'first line\n-export([fake/0]).\nsecond line'.\nreal(X) -> X.\n";
        let points = resume_points(code, &LiteralSpans::scan(code));

        let lines: Vec<&str> = points
            .iter()
            .map(|offset| code[*offset..].lines().next().unwrap_or_default())
            .collect();

        assert!(lines.contains(&"real(X) -> X."), "got {lines:?}");
        assert!(!lines.contains(&"-export([fake/0])."), "got {lines:?}");
        assert!(!lines.contains(&"second line'."), "got {lines:?}");
    }

    /// A quoted atom is a legal function name, so a multiline one may head a
    /// real form. Only offsets strictly inside a literal are rejected.
    #[test]
    fn a_quoted_atom_that_heads_a_form_stays_a_resume_point() {
        let code = "-module(bank).\n'quoted name'(X) -> X.\n";
        let points = resume_points(code, &LiteralSpans::scan(code));

        assert!(points.contains(&code.find('\'').unwrap()), "got {points:?}");
    }

    #[test]
    fn recovery_is_bounded_by_the_parse_budget() {
        let mut code = String::from("-module(bank).\n");
        for index in 0..200 {
            code.push_str(&format!("broken{index}(( ->\n\nf{index}(X) -> X.\n"));
        }
        let tree = parse(&code);

        assert!(tree.root_node().has_error());
        assert!(recover(&code, &tree).trees.len() <= MAX_RECOVERY_PARSES);
    }
}
