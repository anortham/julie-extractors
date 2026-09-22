//! Bash shellspec/bats call-style test extraction.
//!
//! shellspec and bats declare tests as call expressions (`command` nodes in the
//! Bash grammar), not named function declarations:
//!
//! ```bash
//! # shellspec
//! Describe 'math module'
//!   Context 'addition'
//!     It 'adds two numbers'
//!       When call expr 1 + 1
//!       The output should eq 2
//!     End
//!   End
//! End
//!
//! # bats
//! @test "adds two numbers" {
//!     result="$(expr 1 + 1)"
//!     [ "$result" -eq 2 ]
//! }
//! ```
//!
//! Grammar shape (confirmed via live AST probe against tree-sitter-bash 0.25.1):
//! - Node kind: `command`
//! - Callee: `name` **field** (kind `command_name`; text is the bare name,
//!   e.g. `"Describe"` or `"@test"`).
//! - Description arg: first `argument` **field** child whose kind contains
//!   `"string"` — `raw_string` for shellspec single-quoted args
//!   (`'math module'`), `string` for bats double-quoted args
//!   (`"adds two numbers"`). Decoded via `base.decode_string_literal`.
//!
//! Notes on `@test`: bats `@test "name" { }` parses as a `command` node. The
//! `{` is a trailing `word` argument; we stop at the first string argument.
//!
//! Lifecycle note: shellspec's `setup()`/`teardown()` are `function_definition`
//! nodes, not commands; they receive `is_test = true` via the
//! `classify_symbols_by_role` name-heuristic pass and do not need separate
//! materialization here. The lifecycle slice is therefore empty.

use crate::base::body::body_hash;
use crate::base::{BaseExtractor, NormalizedSpan, Symbol, SymbolOptions};
use crate::test_calls::{
    TestCallCategory, TestCallVocab, build_test_call_symbol, classify_call_exact,
};
use tree_sitter::Node;

/// shellspec + bats vocabulary.
/// - `Describe` / `Context` → container (`test_container = true`)
/// - `It` / `Specify` / `Example` / `Feature` / `Scenario` → test case (`is_test = true`)
/// - `@test` (bats) → test case (`is_test = true`)
pub(crate) const BASH_VOCAB: TestCallVocab = TestCallVocab {
    test: &["It", "Specify", "Example", "Feature", "Scenario", "@test"],
    container: &["Describe", "Context"],
    lifecycle: &[], // setup/teardown are function_definitions, handled by name-heuristics
};

/// Materialize a shellspec/bats `command` as a test/container symbol. Returns
/// `None` for any command that is not a recognized shellspec or bats DSL call
/// (e.g. `echo "msg"`, `curl "url"`), so the caller can invoke it for every
/// `command` node and only DSL calls become symbols.
pub(super) fn extract_bash_test_call(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "command" {
        return None;
    }

    // The callee lives in the `name` field of the `command` node.
    let callee_node = node.child_by_field_name("name")?;
    let full_callee = base.get_node_text(&callee_node);
    // Exact match only (#66): a bash command name is a `word` that can contain '.'
    // (invoking a script `It.helper`), a single dotted token a node-kind guard
    // cannot catch (Mech B). Exact match never equates it to the dotless `It`.
    // shellspec/bats keywords (`Describe`, `It`, `@test`, …) are dotless.
    let category = classify_call_exact(&full_callee, &BASH_VOCAB)?;

    let name = match category {
        // Lifecycle: no description string; use the callee name.
        // (vocab has no lifecycle entries, but keep the branch uniform.)
        TestCallCategory::Lifecycle => full_callee.to_string(),
        // Describe / Context / It / @test — first string argument is the description.
        _ => {
            let mut cursor = node.walk();
            let first_str = node
                .children_by_field_name("argument", &mut cursor)
                .find(|c| c.kind().contains("string"))?;
            base.decode_string_literal(&first_str)?
        }
    };

    Some(build_test_call_symbol(
        base,
        &node,
        &full_callee,
        name,
        category,
        parent_id,
    ))
}

/// ShellSpec keywords that open a block closed by `End`.
const SHELLSPEC_BLOCK_OPENERS: &[&str] = &[
    "Describe",
    "Context",
    "ExampleGroup",
    "It",
    "Specify",
    "Example",
    "Feature",
    "Scenario",
    "fDescribe",
    "fContext",
    "fExampleGroup",
    "fIt",
    "fSpecify",
    "fExample",
    "xDescribe",
    "xContext",
    "xExampleGroup",
    "xIt",
    "xSpecify",
    "xExample",
    "Parameters",
    "Mock",
];

/// bats and ShellSpec words that structure a test rather than call project code.
pub(super) const DSL_KEYWORDS: &[&str] = &[
    "}",
    "End",
    "When",
    "The",
    "Assert",
    "Skip",
    "Pending",
    "Todo",
    "Include",
    "Before",
    "After",
    "BeforeEach",
    "AfterEach",
    "BeforeAll",
    "AfterAll",
    "BeforeCall",
    "AfterCall",
    "BeforeRun",
    "AfterRun",
    "Data",
    "Parameters",
    "Mock",
    "Path",
    "File",
    "Dir",
    "Set",
    "Dump",
    "Intercept",
];

fn command_name(base: &BaseExtractor, node: Node) -> Option<String> {
    (node.kind() == "command")
        .then(|| node.child_by_field_name("name"))
        .flatten()
        .map(|name| base.get_node_text(&name))
}

fn has_arguments(node: Node) -> bool {
    let mut cursor = node.walk();
    node.children_by_field_name("argument", &mut cursor)
        .next()
        .is_some()
}

/// The sibling index of the command that closes the DSL block opened at `index`:
/// the `}` of a bats `@test "name" {`, or the matching ShellSpec `End`.
pub(super) fn block_end(base: &BaseExtractor, siblings: &[Node], index: usize) -> Option<usize> {
    let header = siblings[index];
    let name = command_name(base, header)?;
    if name == "@test" {
        let mut cursor = header.walk();
        let opens_brace = header
            .children_by_field_name("argument", &mut cursor)
            .last()
            .is_some_and(|last| base.get_node_text(&last) == "{");
        if !opens_brace {
            return None;
        }
        return (index + 1..siblings.len())
            .find(|&candidate| command_name(base, siblings[candidate]).as_deref() == Some("}"));
    }
    if !SHELLSPEC_BLOCK_OPENERS.contains(&name.as_str()) {
        return None;
    }
    let mut depth = 0usize;
    for (candidate, sibling) in siblings.iter().enumerate().skip(index + 1) {
        match command_name(base, *sibling).as_deref() {
            Some("End") if !has_arguments(*sibling) => {
                if depth == 0 {
                    return Some(candidate);
                }
                depth -= 1;
            }
            Some(opener) if SHELLSPEC_BLOCK_OPENERS.contains(&opener) => depth += 1,
            _ => {}
        }
    }
    None
}

/// A bats/ShellSpec test symbol that spans its header through the closing
/// `}` or `End`, with the block contents as its body.
pub(super) fn extract_bash_test_block(
    base: &mut BaseExtractor,
    siblings: &[Node],
    index: usize,
    end: usize,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let header = siblings[index];
    let closer = siblings[end];
    let header_symbol = extract_bash_test_call(base, header, parent_id)?;
    let body_start = if command_name(base, header).as_deref() == Some("@test") {
        let mut cursor = header.walk();
        header
            .children_by_field_name("argument", &mut cursor)
            .last()?
    } else {
        siblings[index + 1]
    };
    let span = span_between(&header, &closer);
    let mut symbol = base.create_symbol_from_span(
        &header,
        span,
        header_symbol.name,
        header_symbol.kind,
        SymbolOptions {
            signature: header_symbol.signature,
            visibility: header_symbol.visibility,
            parent_id: header_symbol.parent_id,
            metadata: header_symbol.metadata,
            doc_comment: header_symbol.doc_comment,
            annotations: header_symbol.annotations,
        },
    );
    let body = span_between(&body_start, &closer);
    symbol.body_span = Some(body);
    symbol.body_hash = body_hash(&base.content, body, &base.language);
    Some(symbol)
}

fn span_between(start: &Node, end: &Node) -> NormalizedSpan {
    let end_span = NormalizedSpan::from_node(end);
    NormalizedSpan {
        end_line: end_span.end_line,
        end_column: end_span.end_column,
        end_byte: end_span.end_byte,
        ..NormalizedSpan::from_node(start)
    }
}

/// The command a test wrapper runs: `X` in bats `run X` and ShellSpec
/// `When call X` / `When run [command|script|source] X`.
pub(super) fn wrapped_callee<'a>(base: &BaseExtractor, command: Node<'a>) -> Option<Node<'a>> {
    let name = command_name(base, command)?;
    let mut cursor = command.walk();
    let mut arguments = command
        .children_by_field_name("argument", &mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
        .skip_while(|argument| base.get_node_text(argument).starts_with('-'));
    match name.as_str() {
        "run" => arguments.next(),
        "When" => {
            let mode = arguments.next()?;
            if !matches!(base.get_node_text(&mode).as_str(), "call" | "run") {
                return None;
            }
            let target = arguments.next()?;
            if matches!(
                base.get_node_text(&target).as_str(),
                "command" | "script" | "source"
            ) {
                arguments.next()
            } else {
                Some(target)
            }
        }
        _ => None,
    }
    .filter(|target| target.kind() == "word")
}
