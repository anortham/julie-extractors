//! C# conditional compilation: the grammar cannot parse a directive that
//! splits a statement (`#if` between `else if` arms), so directive lines are
//! blanked, and so is every branch after the first of each `#if` group. The
//! `#if` branch stays, which matches the feature-enabled build that most
//! multi-target libraries treat as their primary code.

/// Preprocessor directive names. `#:` file-app directives and `#!` are not
/// preprocessor directives and stay for the grammar.
const DIRECTIVES: &[&str] = &[
    "if",
    "elif",
    "else",
    "endif",
    "define",
    "undef",
    "region",
    "endregion",
    "pragma",
    "nullable",
    "line",
    "error",
    "warning",
];

/// `None` when the source has no directive line.
pub(crate) fn blank_directives(content: &str) -> Option<String> {
    let mut bytes = content.as_bytes().to_vec();
    let mut branch_active: Vec<bool> = Vec::new();
    let mut changed = false;
    let mut line_start = 0;
    // ponytail: a line that starts with `#` inside a verbatim or raw string
    // counts as a directive; track string state if that ever misfires.
    for line in content.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let active = branch_active.last().copied().unwrap_or(true);
        let directive = line.trim_start().strip_prefix('#').map(|rest| {
            rest.trim_start()
                .split(|c: char| !c.is_ascii_alphabetic())
                .next()
                .unwrap_or_default()
        });
        match directive {
            Some("if") => branch_active.push(active),
            Some("elif" | "else") => {
                if let Some(branch) = branch_active.last_mut() {
                    *branch = false;
                }
            }
            Some("endif") => {
                branch_active.pop();
            }
            _ => {}
        }
        let is_preprocessor_directive = directive.is_some_and(|name| DIRECTIVES.contains(&name));
        if is_preprocessor_directive || !active {
            blank(&mut bytes[line_start..line_end]);
            changed = true;
        }
        line_start = line_end;
    }
    changed.then(|| String::from_utf8(bytes).ok()).flatten()
}

fn blank(line: &mut [u8]) {
    for byte in line {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}
