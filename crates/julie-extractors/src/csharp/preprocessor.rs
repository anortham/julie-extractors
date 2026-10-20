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
        let directive = directive_name(line);
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
        if is_directive_line(line) || !active {
            blank(&mut bytes[line_start..line_end]);
            changed = true;
        }
        line_start = line_end;
    }
    changed.then(|| String::from_utf8(bytes).ok()).flatten()
}

fn directive_name(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix('#').map(|rest| {
        rest.trim_start()
            .split(|c: char| !c.is_ascii_alphabetic())
            .next()
            .unwrap_or_default()
    })
}

/// Marks each symbol in the kept branch of an `#if` group with
/// `preprocessorIf`: the condition of that branch, joined with ` && ` under an
/// enclosing kept branch (`!HAVE_X` for a fallback type).
pub(crate) fn mark_conditional_symbols(content: &str, symbols: &mut [crate::base::Symbol]) {
    let branches = kept_branches(content);
    if branches.is_empty() {
        return;
    }
    for symbol in symbols.iter_mut() {
        let at = symbol.start_byte as usize;
        let Some((_, _, condition)) = branches
            .iter()
            .filter(|(start, end, _)| *start <= at && at < *end)
            .max_by_key(|(start, _, _)| *start)
        else {
            continue;
        };
        symbol
            .metadata
            .get_or_insert_with(Default::default)
            .insert("preprocessorIf".to_string(), condition.clone().into());
    }
}

/// The byte range and condition of the first branch of each `#if` group that
/// `blank_directives` keeps.
fn kept_branches(content: &str) -> Vec<(usize, usize, String)> {
    struct Group {
        kept: bool,
        start: usize,
        condition: String,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut branches = Vec::new();
    let mut line_start = 0;
    for line in content.split_inclusive('\n') {
        let line_end = line_start + line.len();
        match directive_name(line) {
            Some("if") => {
                let enclosing = groups.last().filter(|group| group.kept);
                let own = line
                    .trim_start()
                    .trim_start_matches('#')
                    .trim_start()
                    .trim_start_matches("if")
                    .split("//")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let kept = groups.last().is_none_or(|group| group.kept);
                let condition = match enclosing {
                    Some(group) => format!("{} && {own}", group.condition),
                    None => own,
                };
                groups.push(Group {
                    kept,
                    start: line_end,
                    condition,
                });
            }
            Some(name @ ("elif" | "else" | "endif")) => {
                if let Some(group) = groups.last_mut()
                    && group.kept
                {
                    branches.push((group.start, line_start, group.condition.clone()));
                    group.kept = false;
                }
                if name == "endif" {
                    groups.pop();
                }
            }
            _ => {}
        }
        line_start = line_end;
    }
    branches
}

/// True when `blank_directives` blanks the line as a preprocessor directive.
pub(crate) fn is_directive_line(line: &str) -> bool {
    directive_name(line).is_some_and(|name| DIRECTIVES.contains(&name))
}

fn blank(line: &mut [u8]) {
    for byte in line {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}
