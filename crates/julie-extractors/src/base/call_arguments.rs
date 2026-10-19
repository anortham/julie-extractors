use std::collections::HashMap;

use tree_sitter::{Node, Tree, TreeCursor};

use crate::base::{Identifier, IdentifierKind};
use crate::csharp::preprocessor::is_directive_line;

struct ArgumentSummary {
    count: usize,
    spread: bool,
}

/// Adds `arg_count` (and `arg_spread` when one argument is a spread) to the
/// metadata of every call identifier that is the terminal callee name of a call
/// node with an argument list.
pub(crate) fn enrich(language: &str, tree: &Tree, source: &str, identifiers: &mut [Identifier]) {
    let language = grammar_language(language);
    if call_kinds(language).is_empty() {
        return;
    }

    let root = tree.root_node();
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call
            || grammar_language(&identifier.language) != language
        {
            continue;
        }
        let span = (identifier.start_byte as usize, identifier.end_byte as usize);
        let Some(node) = root.named_descendant_for_byte_range(span.0, span.1) else {
            continue;
        };
        let Some(summary) = call_arguments(language, node, span, source.as_bytes()) else {
            continue;
        };
        let metadata = identifier.metadata.get_or_insert_with(HashMap::new);
        metadata.insert("arg_count".to_string(), summary.count.into());
        if summary.spread {
            metadata.insert("arg_spread".to_string(), true.into());
        }
    }
}

fn grammar_language(language: &str) -> &str {
    match crate::language::language_spec(language).map_or(language, |spec| spec.name) {
        "jsx" => "javascript",
        "tsx" => "typescript",
        language => language,
    }
}

fn call_kinds(language: &str) -> &'static [&'static str] {
    match language {
        "python" => &["call"],
        "javascript" | "typescript" => &["call_expression", "new_expression"],
        "java" => &["method_invocation"],
        "csharp" => &[
            "invocation_expression",
            "object_creation_expression",
            "implicit_object_creation_expression",
        ],
        "go" => &["call_expression", "type_conversion_expression"],
        "rust" | "kotlin" | "c" | "cpp" => &["call_expression"],
        _ => &[],
    }
}

fn call_arguments(
    language: &str,
    node: Node<'_>,
    span: (usize, usize),
    source: &[u8],
) -> Option<ArgumentSummary> {
    let mut current = node;
    loop {
        let kind = current.kind();
        if call_kinds(language).contains(&kind) {
            return terminal_callee_arguments(language, current, span, source);
        }
        if ends_the_walk(kind) {
            return None;
        }
        current = current.parent()?;
    }
}

fn ends_the_walk(kind: &str) -> bool {
    matches!(
        kind,
        "macro_invocation"
            | "method_reference"
            | "arrow_function"
            | "function_expression"
            | "func_literal"
            | "closure_expression"
            | "anonymous_function"
    ) || kind.starts_with("jsx_")
        || kind.contains("lambda")
        || [
            "_statement",
            "_declaration",
            "_definition",
            "_item",
            "block",
        ]
        .iter()
        .any(|suffix| kind.ends_with(suffix))
}

fn terminal_callee_arguments(
    language: &str,
    call: Node<'_>,
    span: (usize, usize),
    source: &[u8],
) -> Option<ArgumentSummary> {
    if call.has_error() {
        return None;
    }
    if call.kind() == "type_conversion_expression" {
        let open = call
            .children(&mut call.walk())
            .find(|child| child.kind() == "(")?;
        return is_terminal_name(language, call, span, open.start_byte(), source).then_some(
            ArgumentSummary {
                count: 1,
                spread: false,
            },
        );
    }
    let list = argument_list(language, call)?;
    if !is_terminal_name(language, call, span, list.start_byte(), source) {
        return None;
    }
    summarize(language, call, list, source)
}

/// True when only comments, one optional-call `?.` or Rust `::`, and one
/// type-argument list stand between the identifier and the start of the
/// argument list. The check reads the tokens of the tree, so a string,
/// character, regex or comment inside the type arguments never closes them.
fn is_terminal_name(
    language: &str,
    call: Node<'_>,
    span: (usize, usize),
    list_start: usize,
    source: &[u8],
) -> bool {
    if span.1 > list_start {
        return false;
    }
    let delimiters = type_argument_delimiters(language);
    let open = delimiters.map(|(open, _)| open);
    let close = delimiters.map(|(_, close)| close);
    let mut turbofish = false;
    let mut optional_call = false;
    let mut depth = 0usize;
    let mut nested = 0usize;
    let mut closed = false;
    for token in Tokens::from_byte(call, source, span.1) {
        if token.start_byte() >= list_start {
            break;
        }
        if token.is_extra() {
            continue;
        }
        if closed {
            return false;
        }
        let kind = token.kind();
        let is_leaf = token.child_count() == 0;
        if depth > 0 {
            if !is_leaf {
                continue;
            }
            if nested == 0 && Some(kind) == open {
                depth += 1;
            } else if nested == 0 && Some(kind) == close {
                depth -= 1;
                closed = depth == 0;
            } else {
                match bracket(token, source) {
                    Some(Bracket::Open) => nested += 1,
                    Some(Bracket::Close) => {
                        let Some(remaining) = nested.checked_sub(1) else {
                            return false;
                        };
                        nested = remaining;
                    }
                    None => {}
                }
            }
            continue;
        }
        let is_bracketed_list = !is_leaf
            && token
                .child(0)
                .is_some_and(|first| Some(first.kind()) == open);
        match kind {
            "::" if language == "rust" && !turbofish => turbofish = true,
            "?." | "optional_chain"
                if matches!(language, "javascript" | "typescript") && !optional_call =>
            {
                optional_call = true;
            }
            _ if is_leaf && Some(kind) == open => depth = 1,
            _ if is_bracketed_list => closed = true,
            _ => return false,
        }
    }
    depth == 0 && (closed || !turbofish)
}

fn type_argument_delimiters(language: &str) -> Option<(&'static str, &'static str)> {
    match language {
        "typescript" | "rust" | "csharp" | "cpp" | "kotlin" | "java" => Some(("<", ">")),
        "go" => Some(("[", "]")),
        _ => None,
    }
}

/// The text after leading whitespace and comments. A block comment nests, as
/// in Kotlin, and one that never closes leaves nothing.
fn skip_trivia(mut text: &[u8]) -> &[u8] {
    loop {
        text = text.trim_ascii_start();
        if text.starts_with(b"//") {
            text = text
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(&[][..], |end| &text[end + 1..]);
        } else if text.starts_with(b"/*") {
            let mut depth = 0usize;
            let mut index = 0;
            loop {
                if text[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if text[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
                if depth == 0 {
                    break;
                }
                if index >= text.len() {
                    return &[];
                }
            }
            text = &text[index..];
        } else {
            return text;
        }
    }
}

fn argument_list<'tree>(language: &str, call: Node<'tree>) -> Option<Node<'tree>> {
    match (language, call.kind()) {
        ("kotlin", _) => first_child_of_kind(call, &["value_arguments", "annotated_lambda"]),
        ("csharp", "implicit_object_creation_expression") => {
            first_child_of_kind(call, &["argument_list"])
        }
        _ => call.child_by_field_name("arguments"),
    }
}

fn first_child_of_kind<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| kinds.contains(&child.kind()))
}

fn summarize(
    language: &str,
    call: Node<'_>,
    list: Node<'_>,
    source: &[u8],
) -> Option<ArgumentSummary> {
    let mut summary = match list.kind() {
        "template_string" => return None,
        "generator_expression" => ArgumentSummary {
            count: 1,
            spread: false,
        },
        "annotated_lambda" => ArgumentSummary {
            count: 0,
            spread: false,
        },
        _ => {
            if has_empty_argument_slot(list)
                || (language == "csharp" && holds_preprocessor_directive(call, list, source))
            {
                return None;
            }
            let mut cursor = list.walk();
            let arguments: Vec<_> = list
                .named_children(&mut cursor)
                .filter(|child| !child.is_extra() && child.kind() != "attribute_item")
                .collect();
            if has_angle_ambiguity(language)
                && token_argument_count(language, list, source) != Some(arguments.len())
            {
                return None;
            }
            ArgumentSummary {
                count: arguments.len(),
                spread: arguments.iter().any(|argument| is_spread(*argument)),
            }
        }
    };
    if language == "kotlin" {
        summary.count += kotlin_trailing_lambdas(call, source)?;
    }
    Some(summary)
}

/// True when a comma has no argument before it. JavaScript, TypeScript and TSX
/// parse `f(,a)`, `f(a,, b)` and `f(,)` without an error, and Python and Rust
/// parse `f(,)`; a single trailing comma after an argument is valid.
fn has_empty_argument_slot(list: Node<'_>) -> bool {
    let mut after_argument = false;
    let mut cursor = list.walk();
    for child in list.children(&mut cursor).filter(|child| !child.is_extra()) {
        if child.kind() == "," {
            if !after_argument {
                return true;
            }
            after_argument = false;
        } else {
            after_argument = child.is_named();
        }
    }
    false
}

/// True when a C# directive line starts between the start of the call and the
/// end of its argument list. The parse runs on a copy of the source in which
/// every directive line is blank and only the first branch of each `#if` group
/// is left, so the tree holds one branch and no node for the directive. The
/// blanking also reads a line inside a verbatim string, a raw string or a
/// comment as a directive, and blanking an inactive branch there can merge two
/// leaves, so such a line counts too.
fn holds_preprocessor_directive(call: Node<'_>, list: Node<'_>, source: &[u8]) -> bool {
    let Some(text) = source
        .get(call.start_byte()..list.end_byte())
        .filter(|text| text.contains(&b'#'))
    else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return true;
    };
    text.split_inclusive('\n').any(is_directive_line)
}

fn has_angle_ambiguity(language: &str) -> bool {
    matches!(language, "typescript" | "csharp" | "cpp" | "kotlin" | "c")
}

/// The number of non-empty comma-separated items at the top level of the
/// argument list `list`, read from its tokens, or `None` when the commas can
/// be read two ways.
///
/// TypeScript, C#, C, C++ and Kotlin read `f<A, B>(x)` as two comparisons or as
/// one generic call, and tree-sitter takes one reading without an error node.
/// Whichever reading it takes, a comma that moves between the type arguments
/// and the argument list sits inside an open angle list and before a `>` token
/// at the top level of the list. Round, square and curly brackets do not move,
/// so an angle list never crosses one.
///
/// The angle depth rises by the `<` characters of a token that can open a type
/// list and falls by the `>` characters of a token that can close one, and it
/// stops at 0. A comma at depth 1 or more, followed by any `>` token, gives
/// the list no count. Every `>` between the `<` that opens a list and a comma
/// inside it closes a list opened in between, so the depth never reaches 0
/// before that comma. A token with an `=` (`<=`, `<<=`, `>=`, `>>=`, `>>>=`)
/// opens or closes nothing and has no weight. A comparison that closes before
/// the next comma (`g<A>(x), h<B>(y)`) leaves the depth at 0 and keeps its
/// count.
fn token_argument_count(language: &str, list: Node<'_>, source: &[u8]) -> Option<usize> {
    let mut tokens = Tokens::new(list, source);
    tokens.creation_types_are_tokens = language == "csharp";
    let mut depth = 0usize;
    let mut items = 0usize;
    let mut has_content = false;
    let mut angle_depth = 0usize;
    let mut comma_inside_angles = false;
    for token in tokens.filter(|token| !token.is_extra()) {
        let is_leaf = token.child_count() == 0;
        if is_leaf {
            match bracket(token, source) {
                Some(Bracket::Open) => {
                    depth += 1;
                    has_content |= depth > 1;
                    continue;
                }
                Some(Bracket::Close) => {
                    depth = depth.checked_sub(1)?;
                    continue;
                }
                None => {}
            }
        }
        if depth != 1 {
            continue;
        }
        let kind = token.kind();
        if is_leaf && kind == "," {
            items += usize::from(has_content);
            has_content = false;
            comma_inside_angles |= angle_depth > 0;
            continue;
        }
        has_content = true;
        if !is_leaf {
            continue;
        }
        if comma_inside_angles && is_greater_than_token(kind) {
            return None;
        }
        angle_depth =
            (angle_depth + opening_angles(language, kind)).saturating_sub(closing_angles(kind));
    }
    (depth == 0).then_some(items + usize::from(has_content))
}

/// The `<` characters of the token that can open a type list. TypeScript reads
/// `f<<T>(x: T) => void>(g)` with a type list inside a type list, so `<<` opens
/// two there. In C#, C, C++ and Kotlin no type starts with `<`, so `<<` is a
/// shift and opens none.
fn opening_angles(language: &str, kind: &str) -> usize {
    match kind {
        "<" => 1,
        "<<" if language == "typescript" => 2,
        _ => 0,
    }
}

fn closing_angles(kind: &str) -> usize {
    match kind {
        ">" => 1,
        ">>" => 2,
        ">>>" => 3,
        _ => 0,
    }
}

fn is_greater_than_token(kind: &str) -> bool {
    matches!(kind, ">" | ">=" | ">>" | ">>=" | ">>>" | ">>>=")
}

#[derive(Clone, Copy, PartialEq)]
enum Bracket {
    Open,
    Close,
}

/// Whether the leaf is a round, square or curly bracket, or the opening
/// `${` of a template substitution. The C# grammar names the braces of an
/// interpolation `interpolation_brace` and only the text tells them apart.
fn bracket(leaf: Node<'_>, source: &[u8]) -> Option<Bracket> {
    match leaf.kind() {
        "(" | "[" | "{" | "${" => Some(Bracket::Open),
        ")" | "]" | "}" => Some(Bracket::Close),
        "interpolation_brace" => match source.get(leaf.start_byte()) {
            Some(b'{') => Some(Bracket::Open),
            Some(b'}') => Some(Bracket::Close),
            _ => None,
        },
        _ => None,
    }
}

/// The tokens below a node in source order, walked with a cursor and no
/// recursion. A token is a leaf, a comment or other extra, a group that
/// opens and closes with brackets (`(...)`, `[...]`, `{...}`, `${...}`), or
/// when `creation_types_are_tokens` is set the type of a C# creation. A group
/// or a type holds its own commas and angle brackets and is not entered. Strings,
/// characters, regexes, template text and JSX text are leaves of the tree, so
/// none of their characters is ever a token of its own.
struct Tokens<'tree, 'source> {
    cursor: TreeCursor<'tree>,
    source: &'source [u8],
    first: Option<Node<'tree>>,
    descend: bool,
    creation_types_are_tokens: bool,
}

impl<'tree, 'source> Tokens<'tree, 'source> {
    fn new(root: Node<'tree>, source: &'source [u8]) -> Self {
        Self {
            cursor: root.walk(),
            source,
            first: None,
            descend: true,
            creation_types_are_tokens: false,
        }
    }

    /// Starts at the first token of `root` that ends after `byte`.
    fn from_byte(root: Node<'tree>, source: &'source [u8], byte: usize) -> Self {
        let mut tokens = Self {
            descend: false,
            ..Self::new(root, source)
        };
        while tokens.cursor.goto_first_child_for_byte(byte).is_some() {
            let node = tokens.cursor.node();
            if tokens.is_token(node) {
                tokens.first = Some(node);
                break;
            }
        }
        tokens
    }

    fn is_token(&self, node: Node<'tree>) -> bool {
        node.child_count() == 0
            || node.is_extra()
            || self.is_group(node)
            || self.is_creation_type(node)
    }

    /// The type after `new` in C# (`new Dictionary<A, B>(x)`, `new T<A, B>[3]`).
    /// A type is the only thing that can follow `new`, so its `<`, `>` and
    /// commas are never comparisons.
    fn is_creation_type(&self, node: Node<'tree>) -> bool {
        self.creation_types_are_tokens
            && self.cursor.field_name() == Some("type")
            && node.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "object_creation_expression" | "array_creation_expression"
                )
            })
    }

    fn is_group(&self, node: Node<'tree>) -> bool {
        let count = node.child_count();
        let last = count
            .checked_sub(1)
            .and_then(|index| u32::try_from(index).ok())
            .and_then(|index| node.child(index));
        count >= 2
            && node.child(0).and_then(|first| bracket(first, self.source)) == Some(Bracket::Open)
            && last.and_then(|last| bracket(last, self.source)) == Some(Bracket::Close)
    }
}

impl<'tree> Iterator for Tokens<'tree, '_> {
    type Item = Node<'tree>;

    fn next(&mut self) -> Option<Node<'tree>> {
        if let Some(first) = self.first.take() {
            return Some(first);
        }
        loop {
            if !(self.descend && self.cursor.goto_first_child()) {
                while !self.cursor.goto_next_sibling() {
                    if !self.cursor.goto_parent() {
                        self.descend = false;
                        return None;
                    }
                }
            }
            let node = self.cursor.node();
            self.descend = !self.is_token(node);
            if !self.descend {
                return Some(node);
            }
        }
    }
}

fn is_spread(argument: Node<'_>) -> bool {
    match argument.kind() {
        "list_splat"
        | "dictionary_splat"
        | "spread_element"
        | "variadic_argument"
        | "parameter_pack_expansion" => true,
        "value_argument" => first_child_of_kind(argument, &["spread_expression"]).is_some(),
        _ => false,
    }
}

fn count_children_of_kind(node: Node<'_>, kind: &str) -> usize {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .count()
}

/// The Kotlin grammar parses `f(1) { ... }` as a call to `f(1)` wrapped in a
/// call whose only other child is the lambda. `None` when the lambda is not
/// reliably attached: its wrapper holds a syntax error, a `{` follows the call
/// that the tree left out of it, or the call is a class delegation
/// (`class A : I by f(1) { ... }`) that took braces, which may be the class
/// body. A lambda after a call that already ends in a lambda (`f { a } { b }`)
/// belongs to the result of that call.
fn kotlin_trailing_lambdas(call: Node<'_>, source: &[u8]) -> Option<usize> {
    let own = count_children_of_kind(call, "annotated_lambda");
    let wrapper = call.parent().filter(|parent| {
        parent.kind() == "call_expression"
            && parent.named_child(0) == Some(call)
            && count_children_of_kind(*parent, "annotated_lambda") > 0
    });
    let in_delegation = wrapper
        .unwrap_or(call)
        .parent()
        .is_some_and(|parent| parent.kind() == "explicit_delegation");
    if in_delegation {
        return (own == 0 && wrapper.is_none()).then_some(0);
    }
    if own > 0 {
        return Some(own);
    }
    match wrapper {
        Some(wrapper) if wrapper.has_error() => None,
        Some(wrapper) => Some(count_children_of_kind(wrapper, "annotated_lambda")),
        None => {
            let after = source.get(call.end_byte()..)?;
            (skip_trivia(after).first() != Some(&b'{')).then_some(0)
        }
    }
}
