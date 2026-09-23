//! Auto-property forms the pinned grammar cannot parse. `static member val`
//! and `default val` break the parse of every member after them, while the
//! plain `member val` form parses. The `static` keyword is blanked and
//! `default` is rewritten to `member`, both at the same byte length, so the
//! declaration parses as an auto-property and offsets stay exact.

/// `None` when the source has no such form.
pub(crate) fn rewrite_auto_properties(content: &str) -> Option<String> {
    let mut bytes = content.as_bytes().to_vec();
    let mut changed = false;
    for (pattern, replacement) in [
        ("static member val ", "       member val "),
        ("default val ", "member  val "),
    ] {
        let mut from = 0;
        while let Some(offset) = content[from..].find(pattern) {
            let start = from + offset;
            from = start + pattern.len();
            let at_word_start = start == 0
                || content.as_bytes()[start - 1].is_ascii_whitespace()
                || content.as_bytes()[start - 1] == b']';
            if at_word_start {
                bytes[start..start + pattern.len()].copy_from_slice(replacement.as_bytes());
                changed = true;
            }
        }
    }
    changed.then(|| String::from_utf8(bytes).ok()).flatten()
}
