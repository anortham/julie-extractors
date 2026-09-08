use super::{SyntaxError, SyntaxOptions};
use crate::{NormalizedSpan, ParseDiagnostic, ParseDiagnosticKind};

#[cfg(test)]
pub(crate) type TestDiagnosticNodeHook = Box<dyn FnMut(&tree_sitter::Node)>;

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_DIAGNOSTIC_NODE_HOOK: std::cell::RefCell<Option<TestDiagnosticNodeHook>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn collect(
    tree: &tree_sitter::Tree,
    options: &SyntaxOptions<'_>,
) -> Result<Vec<ParseDiagnostic>, SyntaxError> {
    let mut cursor = tree.walk();
    let mut diagnostics = Vec::new();
    loop {
        options.check()?;
        let node = cursor.node();

        #[cfg(test)]
        TEST_DIAGNOSTIC_NODE_HOOK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(&node);
            }
        });

        for (present, kind) in [
            (node.is_error(), ParseDiagnosticKind::Error),
            (node.is_missing(), ParseDiagnosticKind::Missing),
        ] {
            if present {
                let span = NormalizedSpan::from_node(&node);
                diagnostics.push(ParseDiagnostic {
                    kind,
                    message: None,
                    start_line: span.start_line,
                    start_column: span.start_column,
                    end_line: span.end_line,
                    end_column: span.end_column,
                    start_byte: span.start_byte,
                    end_byte: span.end_byte,
                });
            }
        }

        if node.has_error() && cursor.goto_first_child() {
            continue;
        }

        loop {
            options.check()?;
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(diagnostics);
            }
        }
    }
}
