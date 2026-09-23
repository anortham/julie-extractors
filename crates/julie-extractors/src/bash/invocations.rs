//! Commands that run another command: wrappers (`sudo`, `timeout 30`, `env`,
//! `xargs`, bats `run`, ShellSpec `When call`), `trap` handlers, `complete -F`
//! functions, and ShellSpec hooks. Content-based so the structural-fact
//! collectors can share it with the extractor.

use std::collections::HashSet;
use std::ops::Range;
use tree_sitter::Node;

/// A command that another command runs.
pub(crate) struct Invocation<'a> {
    pub(crate) name: String,
    /// The argument node that names the command.
    pub(crate) anchor: Node<'a>,
    /// The byte range of the name, inside `anchor` for a string handler.
    pub(crate) range: Range<usize>,
    /// The arguments the wrapped command receives; `None` when the command is
    /// only referenced (a trap handler or a hook).
    pub(crate) arguments: Option<Vec<Node<'a>>>,
}

pub(crate) struct CommandScope<'s> {
    pub(crate) local_functions: &'s HashSet<String>,
    pub(crate) test_context: bool,
}

struct WrapperSpec {
    value_flags: &'static [&'static str],
    stop_flags: &'static [&'static str],
    skip_assignments: bool,
    leading_operands: usize,
}

const PLAIN: WrapperSpec = WrapperSpec {
    value_flags: &[],
    stop_flags: &[],
    skip_assignments: false,
    leading_operands: 0,
};

fn wrapper_spec(name: &str, test_context: bool) -> Option<WrapperSpec> {
    Some(match name {
        "sudo" | "doas" => WrapperSpec {
            value_flags: &[
                "-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-U", "-T", "--user", "--group",
            ],
            skip_assignments: true,
            ..PLAIN
        },
        "exec" => WrapperSpec {
            value_flags: &["-a"],
            ..PLAIN
        },
        "time" | "nohup" | "builtin" | "setsid" => PLAIN,
        "timeout" => WrapperSpec {
            value_flags: &["-s", "-k", "--signal", "--kill-after"],
            leading_operands: 1,
            ..PLAIN
        },
        "nice" => WrapperSpec {
            value_flags: &["-n", "--adjustment"],
            ..PLAIN
        },
        "env" => WrapperSpec {
            value_flags: &["-u", "-C", "--unset", "--chdir"],
            skip_assignments: true,
            ..PLAIN
        },
        "command" => WrapperSpec {
            stop_flags: &["-v", "-V"],
            ..PLAIN
        },
        "xargs" => WrapperSpec {
            value_flags: &["-n", "-P", "-I", "-L", "-s", "-d", "-E", "-a"],
            ..PLAIN
        },
        "run" if test_context => WrapperSpec {
            value_flags: &["--keep-empty-lines"],
            ..PLAIN
        },
        _ => return None,
    })
}

const SHELLSPEC_HOOKS: &[&str] = &[
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
];

pub(crate) fn text<'c>(content: &'c str, node: Node<'_>) -> &'c str {
    content.get(node.byte_range()).unwrap_or_default()
}

pub(crate) fn arguments<'a>(command: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = command.walk();
    command
        .children_by_field_name("argument", &mut cursor)
        .collect()
}

/// The static name of a command: a plain word, or a quoted string with no
/// expansion. Dynamic names such as `"$DIR/tool"` or `$cmd` have none.
pub(crate) fn static_command_name<'a>(
    content: &str,
    command: Node<'a>,
) -> Option<(Node<'a>, String)> {
    let name = command.child_by_field_name("name")?;
    let inner = name.named_child(0)?;
    let text = static_word(content, inner)?;
    Some((name, text))
}

/// The text of a word or quoted string that contains no expansion.
pub(crate) fn static_word(content: &str, node: Node<'_>) -> Option<String> {
    let raw = text(content, node);
    let value = match node.kind() {
        "word" | "number" => raw.to_string(),
        "raw_string" => raw.get(1..raw.len().checked_sub(1)?)?.to_string(),
        "string" => {
            let mut cursor = node.walk();
            let mut value = String::new();
            for child in node.named_children(&mut cursor) {
                if child.kind() != "string_content" {
                    return None;
                }
                value.push_str(text(content, child));
            }
            value
        }
        _ => return None,
    };
    (!value.is_empty() && !value.contains(['$', '`'])).then_some(value)
}

/// Every command `command` runs or registers.
pub(crate) fn invocations<'a>(
    content: &str,
    command: Node<'a>,
    scope: &CommandScope<'_>,
) -> Vec<Invocation<'a>> {
    let Some((_, name)) = static_command_name(content, command) else {
        return Vec::new();
    };
    if scope.local_functions.contains(&name) {
        return Vec::new();
    }
    let args = arguments(command);
    match name.as_str() {
        "trap" => trap_handler(content, &args).into_iter().collect(),
        "complete" => args
            .iter()
            .position(|arg| text(content, *arg) == "-F")
            .and_then(|index| args.get(index + 1))
            .and_then(|arg| reference(content, *arg))
            .into_iter()
            .collect(),
        hook if scope.test_context && SHELLSPEC_HOOKS.contains(&hook) => args
            .iter()
            .filter_map(|arg| string_handler(content, *arg))
            .collect(),
        "When" if scope.test_context => shellspec_when(content, &args, scope).into_iter().collect(),
        _ => wrapped_command(content, &name, &args, scope)
            .into_iter()
            .collect(),
    }
}

fn wrapped_command<'a>(
    content: &str,
    name: &str,
    args: &[Node<'a>],
    scope: &CommandScope<'_>,
) -> Option<Invocation<'a>> {
    let mut found = None;
    let mut name = name.to_string();
    let mut rest = args;
    while let Some(spec) = wrapper_spec(&name, scope.test_context) {
        let Some(index) = callee_index(content, rest, &spec) else {
            break;
        };
        let Some(callee) = reference(content, rest[index]) else {
            break;
        };
        rest = &rest[index + 1..];
        name = callee.name.clone();
        found = Some(Invocation {
            arguments: Some(rest.to_vec()),
            ..callee
        });
        if scope.local_functions.contains(&name) {
            break;
        }
    }
    found
}

fn callee_index(content: &str, args: &[Node<'_>], spec: &WrapperSpec) -> Option<usize> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        let word = text(content, *arg);
        if word == "--" {
            index += 1;
            break;
        }
        if word == "!" || (word.starts_with('-') && word.len() > 1) {
            if spec.stop_flags.contains(&word) {
                return None;
            }
            index += if spec.value_flags.contains(&word) {
                2
            } else {
                1
            };
            continue;
        }
        if spec.skip_assignments && is_assignment_word(word) {
            index += 1;
            continue;
        }
        break;
    }
    index += spec.leading_operands;
    (index < args.len()).then_some(index)
}

fn is_assignment_word(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn shellspec_when<'a>(
    content: &str,
    args: &[Node<'a>],
    scope: &CommandScope<'_>,
) -> Option<Invocation<'a>> {
    let mode = args.first()?;
    if !matches!(text(content, *mode), "call" | "run") {
        return None;
    }
    let mut index = 1;
    if matches!(
        args.get(index).map(|arg| text(content, *arg)),
        Some("command" | "script" | "source")
    ) {
        index += 1;
    }
    wrapped_command(content, "run", args.get(index..)?, scope)
}

fn trap_handler<'a>(content: &str, args: &[Node<'a>]) -> Option<Invocation<'a>> {
    let mut args = args.iter();
    let mut handler = args.next()?;
    if text(content, *handler) == "--" {
        handler = args.next()?;
    }
    if text(content, *handler).starts_with('-') {
        return None;
    }
    string_handler(content, *handler)
}

/// A command word argument that names a command directly.
fn reference<'a>(content: &str, node: Node<'a>) -> Option<Invocation<'a>> {
    if node.kind() != "word" {
        return None;
    }
    let name = text(content, node);
    is_command_word(name).then(|| Invocation {
        name: name.to_string(),
        anchor: node,
        range: node.byte_range(),
        arguments: None,
    })
}

/// The first command word of a handler argument: a bare word, or the first
/// word of a quoted command string such as `'notify "x"; exit 1'`.
fn string_handler<'a>(content: &str, node: Node<'a>) -> Option<Invocation<'a>> {
    if node.kind() == "word" {
        return reference(content, node);
    }
    if !matches!(node.kind(), "raw_string" | "string") {
        return None;
    }
    let raw = text(content, node);
    let inner = raw.get(1..raw.len().checked_sub(1)?)?;
    let leading = inner.len() - inner.trim_start().len();
    let word_len = inner[leading..]
        .find(|c: char| c.is_whitespace() || ";&|()<>'\"`".contains(c))
        .unwrap_or(inner.len() - leading);
    let name = &inner[leading..leading + word_len];
    if !is_command_word(name) {
        return None;
    }
    let start = node.start_byte() + 1 + leading;
    Some(Invocation {
        name: name.to_string(),
        anchor: node,
        range: start..start + word_len,
        arguments: None,
    })
}

fn is_command_word(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '_' | '.' | '/'))
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '+'))
        && word != "."
        && word != "-"
}
