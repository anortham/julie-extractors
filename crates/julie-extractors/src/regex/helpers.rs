/// Check if text is a valid regex pattern
#[cfg(test)]
pub(crate) fn is_valid_regex_pattern(text: &str) -> bool {
    // Skip very short patterns or obvious non-regex content
    if text.is_empty() {
        return false;
    }

    // Allow simple literals (letters, numbers, basic words)
    if text.chars().all(|c| c.is_alphanumeric()) {
        return true;
    }

    // Allow single character regex metacharacters
    if matches!(text, "." | "^" | "$") {
        return true;
    }

    // Allow simple groups and common patterns
    if (text.starts_with('(') && text.ends_with(')')) || text.ends_with('*') || text == "**" {
        return true;
    }

    // Check for regex-specific characters or patterns
    let regex_indicators = [
        r"[\[\](){}*+?^$|\\]", // Special regex characters
        r"\\[dwsWDSnrtfve]",   // Escape sequences
        r"\(\?\<?[!=]",        // Lookarounds
        r"\(\?\w+\)",          // Groups with modifiers
        r"\\p\{",              // Unicode properties
        r"\[\^",               // Negated character classes
        r"\{[\d,]+\}",         // Quantifiers
    ];

    regex_indicators.iter().any(|pattern| {
        // Simple pattern matching - check for common regex constructs
        match *pattern {
            r"[\[\](){}*+?^$|\\]" => text.chars().any(|c| "[](){}*+?^$|\\".contains(c)),
            r"\\[dwsWDSnrtfve]" => {
                text.contains(r"\d")
                    || text.contains(r"\w")
                    || text.contains(r"\s")
                    || text.contains(r"\D")
                    || text.contains(r"\W")
                    || text.contains(r"\S")
                    || text.contains(r"\n")
                    || text.contains(r"\r")
                    || text.contains(r"\t")
                    || text.contains(r"\f")
                    || text.contains(r"\v")
                    || text.contains(r"\e")
            }
            r"\(\?\<?[!=]" => {
                text.contains("(?=")
                    || text.contains("(?!")
                    || text.contains("(?<=")
                    || text.contains("(?<!")
            }
            r"\(\?\w+\)" => text.contains("(?") && text.contains(')'),
            r"\\p\{" => text.contains(r"\p{") || text.contains(r"\P{"),
            r"\[\^" => text.contains("[^"),
            r"\{[\d,]+\}" => {
                text.contains('{')
                    && text.contains('}')
                    && text.chars().any(|c| c.is_ascii_digit())
                    && (text.contains(',')
                        || text.chars().filter(|c| c.is_ascii_digit()).count() > 0)
            }
            _ => false,
        }
    })
}

/// Calculate complexity score of a pattern
pub(crate) fn calculate_complexity(pattern: &str) -> u32 {
    let mut complexity = 0;

    // Basic complexity indicators
    complexity += pattern.matches(['*', '+', '?']).count() as u32; // Quantifiers
    complexity += pattern.matches(['[', ']', '(', ')', '{', '}']).count() as u32; // Grouping constructs
    complexity += pattern.matches("(?").count() as u32 * 2; // Lookarounds
    complexity += pattern.matches(r"\p{").count() as u32; // Unicode properties
    complexity += pattern.matches('|').count() as u32; // Alternations

    complexity
}

/// Returns the smallest symbol whose byte range holds the whole node.
///
/// Regex constructs sit side by side with no separator, so a group's end
/// column equals the next construct's start column; line/column containment
/// with an inclusive end would attach a following backreference to the group.
pub(super) fn innermost_symbol<'a>(
    symbols: &'a [crate::base::Symbol],
    node: tree_sitter::Node,
) -> Option<&'a crate::base::Symbol> {
    innermost_symbol_for_bytes(symbols, node.start_byte() as u32, node.end_byte() as u32)
}

pub(super) fn innermost_symbol_for_bytes(
    symbols: &[crate::base::Symbol],
    start: u32,
    end: u32,
) -> Option<&crate::base::Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.start_byte <= start && end <= symbol.end_byte)
        .min_by_key(|symbol| {
            (
                symbol.end_byte - symbol.start_byte,
                symbol.parent_id.is_none(),
            )
        })
}
