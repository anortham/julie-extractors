use crate::base::BodySpan;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone)]
struct IndexedToken {
    start: usize,
    end: usize,
    value: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HashSyntax {
    pub(crate) comments: Vec<Range<usize>>,
    pub(crate) strings: Vec<HashString>,
}

#[derive(Clone, Debug)]
pub(crate) struct HashString {
    pub(crate) range: Range<usize>,
    pub(crate) token_start: usize,
    pub(crate) interpolated: bool,
    pub(crate) code: Vec<Range<usize>>,
}

#[derive(Clone)]
struct IndexedString {
    syntax: HashString,
    normalized: Option<String>,
}

#[derive(Default)]
struct BuildWork {
    #[cfg(test)]
    syntax_nodes: usize,
    #[cfg(test)]
    source_steps: usize,
    #[cfg(test)]
    copied_bytes: usize,
    #[cfg(test)]
    search_steps: usize,
    #[cfg(test)]
    sort_comparisons: usize,
    #[cfg(test)]
    query_steps: usize,
}

impl BuildWork {
    fn visit_node(&mut self) {
        #[cfg(test)]
        {
            self.syntax_nodes += 1;
        }
    }

    fn scan_step(&mut self) {
        #[cfg(test)]
        {
            self.source_steps += 1;
        }
    }

    fn scan_bytes(&mut self, byte_count: usize) {
        #[cfg(test)]
        {
            self.copied_bytes += byte_count;
        }
        #[cfg(not(test))]
        let _ = byte_count;
    }

    fn hash_bytes(&mut self, byte_count: usize) {
        #[cfg(test)]
        {
            self.source_steps += byte_count;
        }
        #[cfg(not(test))]
        let _ = byte_count;
    }

    fn search_step(&mut self) {
        #[cfg(test)]
        {
            self.search_steps += 1;
        }
    }

    fn sort_comparison(&mut self) {
        #[cfg(test)]
        {
            self.sort_comparisons += 1;
        }
    }

    fn query_step(&mut self) {
        #[cfg(test)]
        {
            self.query_steps += 1;
        }
    }

    #[cfg(test)]
    fn units(&self) -> usize {
        self.syntax_nodes
            + self.source_steps
            + self.copied_bytes
            + self.search_steps
            + self.sort_comparisons
    }

    #[cfg(test)]
    fn query_units(&self) -> usize {
        self.query_steps + self.search_steps + self.source_steps + self.copied_bytes
    }
}

pub(crate) struct SyntaxBodyHashIndex {
    source: Arc<str>,
    comments: Vec<Range<usize>>,
    strings: Vec<IndexedString>,
    tokens: Vec<IndexedToken>,
    #[cfg(test)]
    build_work: usize,
}

impl SyntaxBodyHashIndex {
    pub(crate) fn new(source: impl Into<Arc<str>>, syntax: HashSyntax) -> Self {
        let source = source.into();
        let mut work = BuildWork::default();
        let mut comments = syntax
            .comments
            .into_iter()
            .filter(|range| {
                work.visit_node();
                valid_range(source.as_ref(), range)
            })
            .collect::<Vec<_>>();
        comments.sort_unstable_by(|left, right| {
            work.sort_comparison();
            (left.start, left.end).cmp(&(right.start, right.end))
        });
        let mut merged_comments: Vec<Range<usize>> = Vec::with_capacity(comments.len());
        for range in comments {
            work.scan_step();
            if let Some(previous) = merged_comments
                .last_mut()
                .filter(|previous| range.start <= previous.end)
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged_comments.push(range);
            }
        }

        let mut strings = syntax
            .strings
            .into_iter()
            .filter_map(|syntax| {
                work.visit_node();
                valid_string(source.as_ref(), &syntax).then_some(IndexedString {
                    syntax,
                    normalized: None,
                })
            })
            .collect::<Vec<_>>();
        strings.sort_unstable_by(|left, right| {
            work.sort_comparison();
            (
                left.syntax.range.start,
                std::cmp::Reverse(left.syntax.range.end),
            )
                .cmp(&(
                    right.syntax.range.start,
                    std::cmp::Reverse(right.syntax.range.end),
                ))
        });

        let mut normalization_order: Vec<_> = strings
            .iter()
            .enumerate()
            .filter_map(|(index, string)| string.syntax.interpolated.then_some(index))
            .collect();
        normalization_order.sort_unstable_by(|left, right| {
            work.sort_comparison();
            strings[*left]
                .syntax
                .range
                .len()
                .cmp(&strings[*right].syntax.range.len())
        });
        for index in normalization_order {
            let normalized = normalized_interpolated_string(
                source.as_ref(),
                &strings[index].syntax,
                &merged_comments,
                &strings,
                &mut work,
            );
            strings[index].normalized = Some(normalized);
        }

        let mut tokens = Vec::new();
        tokenize_range(
            source.as_ref(),
            0..source.len(),
            &merged_comments,
            &strings,
            &mut tokens,
            &mut work,
        );

        Self {
            source,
            comments: merged_comments,
            strings,
            tokens,
            #[cfg(test)]
            build_work: work.units(),
        }
    }

    pub(crate) fn hash(&self, span: BodySpan) -> Option<String> {
        self.hash_with_counter(span, &mut BuildWork::default())
    }

    #[cfg(test)]
    pub(crate) fn hash_with_work(&self, span: BodySpan) -> (Option<String>, usize) {
        let mut work = BuildWork::default();
        let hash = self.hash_with_counter(span, &mut work);
        (hash, work.query_units())
    }

    fn hash_with_counter(&self, span: BodySpan, work: &mut BuildWork) -> Option<String> {
        let start = span.start_byte as usize;
        let end = span.end_byte as usize;
        if start > end || end > self.source.len() || self.source.get(start..end).is_none() {
            return None;
        }

        let first = self.tokens.partition_point(|token| {
            work.query_step();
            token.end <= start
        });
        let overlaps_token_boundary = self.tokens[first..]
            .iter()
            .take_while(|token| {
                work.query_step();
                token.start < end
            })
            .any(|token| token.start < start || token.end > end);
        if overlaps_token_boundary {
            let mut tokens = Vec::new();
            tokenize_range(
                self.source.as_ref(),
                start..end,
                &self.comments,
                &self.strings,
                &mut tokens,
                work,
            );
            return Some(hash_token_range(&tokens, 0, usize::MAX, work));
        }

        Some(hash_token_range(&self.tokens, first, end, work))
    }

    #[cfg(test)]
    pub(crate) fn build_work(&self) -> usize {
        self.build_work
    }

    #[cfg(test)]
    pub(crate) fn token_count(&self) -> usize {
        self.tokens.len()
    }
}

fn valid_range(source: &str, range: &Range<usize>) -> bool {
    range.start < range.end && source.get(range.clone()).is_some()
}

fn valid_string(source: &str, string: &HashString) -> bool {
    if !valid_range(source, &string.range)
        || string.token_start < string.range.start
        || string.token_start >= string.range.end
        || source.get(string.range.start..string.token_start).is_none()
        || (!string.interpolated && !string.code.is_empty())
    {
        return false;
    }

    let mut previous_end = string.token_start;
    string.code.iter().all(|range| {
        let valid = range.start >= previous_end
            && range.end >= range.start
            && range.end <= string.range.end
            && source.get(range.clone()).is_some();
        previous_end = range.end;
        valid
    })
}

fn hash_token_range(
    tokens: &[IndexedToken],
    first: usize,
    end: usize,
    work: &mut BuildWork,
) -> String {
    let mut input = String::new();
    let mut first_token = true;
    for token in &tokens[first..] {
        work.query_step();
        if token.start >= end {
            break;
        }
        if !first_token {
            input.push('\u{1f}');
        }
        work.scan_bytes(token.value.len());
        input.push_str(&token.value);
        first_token = false;
    }
    format!("{:x}", md5::compute(input.as_bytes()))
}

fn tokenize_range(
    source: &str,
    range: Range<usize>,
    comments: &[Range<usize>],
    strings: &[IndexedString],
    tokens: &mut Vec<IndexedToken>,
    work: &mut BuildWork,
) {
    if range.start >= range.end || source.get(range.clone()).is_none() {
        return;
    }

    let mut cursor = range.start;
    let mut comment_index = comments.partition_point(|comment| {
        work.search_step();
        comment.end <= cursor
    });
    let mut string_index = strings.partition_point(|string| {
        work.search_step();
        string.syntax.range.start < cursor
    });

    while cursor < range.end {
        work.scan_step();
        while comments
            .get(comment_index)
            .is_some_and(|comment| comment.end <= cursor)
        {
            comment_index += 1;
        }
        while strings
            .get(string_index)
            .is_some_and(|string| string.syntax.range.end <= cursor)
        {
            string_index += 1;
        }
        if let Some(comment) = comments.get(comment_index)
            && comment.start <= cursor
            && cursor < comment.end
        {
            cursor = comment.end.min(range.end);
            comment_index += 1;
            continue;
        }

        if let Some(string) = strings.get(string_index)
            && string.syntax.range.start == cursor
            && string.syntax.range.end <= range.end
        {
            tokenize_interpolation_prefix(source, cursor..string.syntax.token_start, tokens, work);
            let value = if let Some(normalized) = &string.normalized {
                normalized.clone()
            } else if let Some(raw) = source.get(string.syntax.token_start..string.syntax.range.end)
            {
                raw.to_owned()
            } else {
                cursor = string.syntax.range.end;
                string_index += 1;
                continue;
            };
            work.scan_bytes(value.len());
            tokens.push(IndexedToken {
                start: string.syntax.token_start,
                end: string.syntax.range.end,
                value,
            });
            cursor = string.syntax.range.end;
            string_index += 1;
            continue;
        }

        if let Some(comment) = comments.get(comment_index)
            && comment.start == cursor
        {
            cursor = comment.end.min(range.end);
            comment_index += 1;
            continue;
        }

        let Some(ch) = source.get(cursor..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        if ch.is_whitespace() {
            cursor += ch.len_utf8();
            continue;
        }

        if matches!(ch, '"' | '\'' | '`') {
            let end = quoted_end(source, cursor, ch, range.end, work);
            work.scan_bytes(end - cursor);
            tokens.push(IndexedToken {
                start: cursor,
                end,
                value: source[cursor..end].to_string(),
            });
            cursor = end;
            continue;
        }

        if is_word_char(ch) {
            let start = cursor;
            cursor += ch.len_utf8();
            while cursor < range.end {
                let Some(next) = source.get(cursor..).and_then(|tail| tail.chars().next()) else {
                    break;
                };
                if !is_word_char(next) {
                    break;
                }
                work.scan_step();
                cursor += next.len_utf8();
            }
            work.scan_bytes(cursor - start);
            tokens.push(IndexedToken {
                start,
                end: cursor,
                value: source[start..cursor].to_string(),
            });
            continue;
        }

        let end = cursor + ch.len_utf8();
        work.scan_bytes(end - cursor);
        tokens.push(IndexedToken {
            start: cursor,
            end,
            value: source[cursor..end].to_string(),
        });
        cursor = end;
    }
}

fn tokenize_interpolation_prefix(
    source: &str,
    range: Range<usize>,
    tokens: &mut Vec<IndexedToken>,
    work: &mut BuildWork,
) {
    for (offset, ch) in source[range.clone()].char_indices() {
        work.scan_step();
        if ch.is_whitespace() {
            continue;
        }
        let start = range.start + offset;
        let end = start + ch.len_utf8();
        work.scan_bytes(end - start);
        tokens.push(IndexedToken {
            start,
            end,
            value: source[start..end].to_string(),
        });
    }
}

fn normalized_interpolated_string(
    source: &str,
    string: &HashString,
    comments: &[Range<usize>],
    strings: &[IndexedString],
    work: &mut BuildWork,
) -> String {
    let mut normalized = String::new();
    let mut cursor = string.token_start;

    for code in &string.code {
        work.scan_step();
        work.scan_bytes(code.start - cursor);
        normalized.push_str(&source[cursor..code.start]);
        append_normalized_code(
            source,
            code.clone(),
            comments,
            strings,
            &mut normalized,
            work,
        );
        cursor = code.end;
    }

    work.scan_bytes(string.range.end - cursor);
    normalized.push_str(&source[cursor..string.range.end]);
    let prefix = source
        .get(string.range.start..string.token_start)
        .unwrap_or_default();
    work.hash_bytes(normalized.len());
    let digest = md5::compute(normalized.as_bytes());
    let value = format!(
        "interpolated_string:{}:{}:{:x}",
        prefix.len(),
        prefix,
        digest
    );
    work.scan_bytes(value.len());
    value
}

fn append_normalized_code(
    source: &str,
    range: Range<usize>,
    comments: &[Range<usize>],
    strings: &[IndexedString],
    normalized: &mut String,
    work: &mut BuildWork,
) {
    let mut tokens = Vec::new();
    tokenize_range(source, range, comments, strings, &mut tokens, work);
    normalized.push('\u{1e}');
    for token in tokens {
        work.scan_step();
        work.scan_bytes(token.value.len());
        normalized.push_str(&token.value.len().to_string());
        normalized.push(':');
        normalized.push_str(&token.value);
        normalized.push('\u{1f}');
    }
    normalized.push('\u{1e}');
}

fn quoted_end(
    source: &str,
    start: usize,
    quote: char,
    limit: usize,
    work: &mut BuildWork,
) -> usize {
    let mut cursor = start + quote.len_utf8();
    let mut escaped = false;
    while cursor < limit {
        work.scan_step();
        let Some(ch) = source.get(cursor..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        cursor += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            break;
        }
    }
    cursor
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::{HashString, HashSyntax, SyntaxBodyHashIndex};
    use crate::base::BodySpan;

    fn span(source: &str, start: usize, end: usize) -> BodySpan {
        BodySpan::from_content_range(source, start, end).expect("source span")
    }

    fn full_span(source: &str) -> BodySpan {
        span(source, 0, source.len())
    }

    fn interpolation(source: &str, comments: Vec<std::ops::Range<usize>>) -> HashSyntax {
        let opening = source.find('{').expect("interpolation opening");
        let closing = source.rfind('}').expect("interpolation closing");
        HashSyntax {
            comments,
            strings: vec![HashString {
                range: 0..source.len(),
                token_start: 1,
                interpolated: true,
                code: vec![std::ops::Range {
                    start: opening + 1,
                    end: closing,
                }],
            }],
        }
    }

    #[test]
    fn comment_descriptors_remove_only_comment_bytes() {
        let source = "left /* ignored */ + right";
        let comment_start = source.find("/*").expect("comment start");
        let comment_end = source.find("*/").expect("comment end") + 2;
        let with_comment = SyntaxBodyHashIndex::new(
            source,
            HashSyntax {
                comments: vec![std::ops::Range {
                    start: comment_start,
                    end: comment_end,
                }],
                strings: Vec::new(),
            },
        );
        let without_comment_source = "left  + right";
        let without_comment =
            SyntaxBodyHashIndex::new(without_comment_source, HashSyntax::default());
        assert_eq!(
            with_comment.hash(full_span(source)),
            without_comment.hash(full_span(without_comment_source))
        );
        assert_ne!(
            with_comment.hash(full_span(source)),
            SyntaxBodyHashIndex::new("left - right", HashSyntax::default())
                .hash(full_span("left - right"))
        );
    }

    #[test]
    fn interpolation_normalizes_code_and_keeps_literal_bytes_significant() {
        let plain = "$\"label { a + b }\"";
        let comment = "$\"label { a /* ignored */ + b }\"";
        let comment_start = comment.find("/*").expect("comment start");
        let comment_end = comment.find("*/").expect("comment end") + 2;
        let plain_hash = SyntaxBodyHashIndex::new(plain, interpolation(plain, Vec::new()))
            .hash(full_span(plain));
        let comment_hash = SyntaxBodyHashIndex::new(
            comment,
            interpolation(
                comment,
                vec![std::ops::Range {
                    start: comment_start,
                    end: comment_end,
                }],
            ),
        )
        .hash(full_span(comment));
        assert_eq!(plain_hash, comment_hash);

        let changed_literal = "$\"label  { a + b }\"";
        let changed_code = "$\"label { a - b }\"";
        assert_ne!(
            plain_hash,
            SyntaxBodyHashIndex::new(changed_literal, interpolation(changed_literal, Vec::new()))
                .hash(full_span(changed_literal))
        );
        assert_ne!(
            plain_hash,
            SyntaxBodyHashIndex::new(changed_code, interpolation(changed_code, Vec::new()))
                .hash(full_span(changed_code))
        );
    }

    #[test]
    fn partial_queries_retokenize_interpolated_code_with_comment_context() {
        let source = "$\"label { a /* ignored */ + b }\"";
        let code_start = source.find('{').expect("interpolation opening") + 1;
        let code_end = source.rfind('}').expect("interpolation closing");
        let comment_start = source.find("/*").expect("comment start");
        let comment_end = source.find("*/").expect("comment end") + 2;
        let index = SyntaxBodyHashIndex::new(
            source,
            interpolation(
                source,
                vec![std::ops::Range {
                    start: comment_start,
                    end: comment_end,
                }],
            ),
        );
        let selected = index.hash(span(source, code_start, code_end));
        let plain_source = "a + b";
        let plain = SyntaxBodyHashIndex::new(plain_source, HashSyntax::default())
            .hash(full_span(plain_source));
        assert_eq!(selected, plain);
    }

    #[test]
    fn opaque_literal_descriptors_keep_the_raw_token_and_literal_sensitivity() {
        for source in ["\"plain\"", "@\"plain\""] {
            let token_start = source.find('"').expect("quote start");
            let syntax = HashSyntax {
                comments: Vec::new(),
                strings: vec![HashString {
                    range: 0..source.len(),
                    token_start,
                    interpolated: false,
                    code: Vec::new(),
                }],
            };
            assert_eq!(
                SyntaxBodyHashIndex::new(source, syntax).hash(full_span(source)),
                SyntaxBodyHashIndex::new(source, HashSyntax::default()).hash(full_span(source))
            );
        }

        let source = "\"plain\"";
        let changed = "\"plain \"";
        let syntax = |source: &str| HashSyntax {
            comments: Vec::new(),
            strings: vec![HashString {
                range: 0..source.len(),
                token_start: 0,
                interpolated: false,
                code: Vec::new(),
            }],
        };
        assert_ne!(
            SyntaxBodyHashIndex::new(source, syntax(source)).hash(full_span(source)),
            SyntaxBodyHashIndex::new(changed, syntax(changed)).hash(full_span(changed))
        );
    }

    #[test]
    fn invalid_descriptors_and_utf8_split_spans_fail_safely() {
        let source = "λ + value";
        let no_descriptors = SyntaxBodyHashIndex::new(source, HashSyntax::default());
        let invalid_descriptor = SyntaxBodyHashIndex::new(
            source,
            HashSyntax {
                comments: vec![std::ops::Range { start: 1, end: 4 }],
                strings: vec![HashString {
                    range: 0..source.len(),
                    token_start: source.len(),
                    interpolated: true,
                    code: Vec::new(),
                }],
            },
        );
        assert_eq!(
            invalid_descriptor.hash(full_span(source)),
            no_descriptors.hash(full_span(source))
        );

        let mut split_utf8 = full_span(source);
        split_utf8.start_byte = 1;
        assert_eq!(no_descriptors.hash(split_utf8), None);

        let mut reversed = full_span(source);
        reversed.start_byte = 4;
        reversed.end_byte = 2;
        assert_eq!(no_descriptors.hash(reversed), None);

        let mut outside = full_span(source);
        outside.end_byte = source.len() as u32 + 1;
        assert_eq!(no_descriptors.hash(outside), None);
    }

    #[test]
    fn empty_interpolations_keep_interpolation_token_framing() {
        let source = "$\"plain\"";
        let syntax = HashSyntax {
            comments: Vec::new(),
            strings: vec![HashString {
                range: 0..source.len(),
                token_start: 1,
                interpolated: true,
                code: Vec::new(),
            }],
        };
        assert_ne!(
            SyntaxBodyHashIndex::new(source, syntax).hash(full_span(source)),
            SyntaxBodyHashIndex::new(source, HashSyntax::default()).hash(full_span(source))
        );
    }
}
