use grapha_core::graph::{NodeKind, Span};

pub fn should_extract_snippet(kind: NodeKind) -> bool {
    !matches!(
        kind,
        NodeKind::Field | NodeKind::Variant | NodeKind::View | NodeKind::Branch
    )
}

pub fn trim_snippet_indentation(snippet: &str) -> String {
    let lines: Vec<&str> = snippet.lines().collect();
    let min_indent = lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_end();
            (!trimmed.is_empty())
                .then_some(line.chars().take_while(|ch| ch.is_whitespace()).count())
        })
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            if line.trim_end().is_empty() {
                String::new()
            } else {
                line.chars()
                    .skip(min_indent)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_string()
}

/// Pre-computed line index for a source file. Build once, query many times.
pub struct LineIndex<'a> {
    source: &'a str,
    /// Byte offsets of the start of each line.
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' && i + 1 < source.len() {
                line_starts.push(i + 1);
            }
        }
        Self {
            source,
            line_starts,
        }
    }

    fn normalize_line(&self, line: usize, one_based: bool) -> Option<usize> {
        if one_based {
            line.checked_sub(1)
        } else {
            Some(line)
        }
    }

    fn byte_offset(&self, line: usize, column: usize) -> Option<usize> {
        let line_start = *self.line_starts.get(line)?;
        let line_limit = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.source.len());
        Some((line_start + column).min(line_limit))
    }

    fn extract_exact_span_with_base(&self, span: &Span, one_based_lines: bool) -> Option<String> {
        let start_line = self.normalize_line(span.start[0], one_based_lines)?;
        let end_line = self.normalize_line(span.end[0], one_based_lines)?;
        let byte_start = self.byte_offset(start_line, span.start[1])?;
        let byte_end = self.byte_offset(end_line, span.end[1])?;
        if byte_start > byte_end || byte_end > self.source.len() {
            return None;
        }

        let slice = &self.source.as_bytes()[byte_start..byte_end];
        let snippet = String::from_utf8_lossy(slice)
            .trim_end_matches(['\n', '\r'])
            .to_string();
        if snippet.is_empty() {
            None
        } else {
            Some(snippet)
        }
    }

    fn declaration_line_for_symbol(&self, symbol: &str, kind: NodeKind) -> Option<String> {
        let symbol = symbol.trim();
        if symbol.is_empty() {
            return None;
        }

        self.source.lines().find_map(|line| {
            let trimmed = line.trim_end();
            declaration_matches_symbol(trimmed, symbol, kind).then(|| trimmed.to_string())
        })
    }

    fn declaration_block_for_symbol(&self, symbol: &str, kind: NodeKind) -> Option<String> {
        if kind != NodeKind::Function {
            return None;
        }

        let symbol = symbol.trim();
        if symbol.is_empty() {
            return None;
        }

        let lines: Vec<&str> = self.source.lines().collect();
        let start_idx = lines
            .iter()
            .position(|line| declaration_matches_symbol(line.trim_end(), symbol, kind))?;

        let mut collected = Vec::new();
        let mut brace_depth = 0usize;
        let mut saw_open_brace = false;

        for line in lines.iter().skip(start_idx) {
            let trimmed = line.trim_end();
            collected.push(*line);

            for ch in trimmed.chars() {
                match ch {
                    '{' => {
                        saw_open_brace = true;
                        brace_depth += 1;
                    }
                    '}' => {
                        brace_depth = brace_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }

            if saw_open_brace && brace_depth == 0 {
                return Some(trim_snippet_indentation(&collected.join("\n")));
            }
        }

        None
    }

    fn score_candidate(
        candidate: &str,
        symbol: &str,
        kind: NodeKind,
        preferred_line: usize,
    ) -> (usize, usize, usize, usize) {
        let trimmed = candidate.trim();
        let symbol_match = usize::from(!symbol.is_empty() && trimmed.contains(symbol));
        let kind_match = usize::from(match kind {
            NodeKind::Function => {
                trimmed.contains("func ")
                    || trimmed.contains("init(")
                    || trimmed.contains("subscript")
                    || trimmed.contains("var ")
            }
            NodeKind::Property | NodeKind::Field | NodeKind::Constant => {
                trimmed.contains("var ") || trimmed.contains("let ")
            }
            NodeKind::Struct => trimmed.contains("struct "),
            NodeKind::Trait => trimmed.contains("trait "),
            NodeKind::Impl => trimmed.contains("impl "),
            NodeKind::Enum => trimmed.contains("enum "),
            NodeKind::Protocol => trimmed.contains("protocol "),
            NodeKind::Extension => trimmed.contains("extension "),
            _ => true,
        });
        let body_match = usize::from(
            kind == NodeKind::Function && trimmed.contains('{') && trimmed.contains('}'),
        );
        let distance = candidate
            .lines()
            .enumerate()
            .find_map(|(idx, line)| {
                line.contains(symbol)
                    .then_some(idx.abs_diff(preferred_line))
            })
            .unwrap_or(usize::MAX / 4);

        (
            symbol_match,
            kind_match,
            body_match,
            usize::MAX.saturating_sub(distance),
        )
    }

    fn extract_full_lines_with_base(&self, span: &Span, one_based_lines: bool) -> Option<String> {
        let start_line = self.normalize_line(span.start[0], one_based_lines)?;
        let end_line = self.normalize_line(span.end[0], one_based_lines)?;

        if start_line >= self.line_starts.len() {
            return None;
        }

        let end_line = end_line.min(self.line_starts.len().saturating_sub(1));
        let byte_start = self.line_starts[start_line];
        let byte_end = if end_line + 1 < self.line_starts.len() {
            self.line_starts[end_line + 1].saturating_sub(1)
        } else {
            self.source.len()
        };

        let slice = &self.source.as_bytes()[byte_start..byte_end];
        Some(
            String::from_utf8_lossy(slice)
                .trim_end_matches(['\n', '\r'])
                .to_string(),
        )
    }

    pub fn extract_full_snippet(&self, span: &Span) -> Option<String> {
        self.extract_exact_span_with_base(span, false)
            .or_else(|| self.extract_exact_span_with_base(span, true))
            .or_else(|| self.extract_full_lines_with_base(span, false))
            .or_else(|| self.extract_full_lines_with_base(span, true))
    }

    pub fn extract_symbol_snippet(
        &self,
        span: &Span,
        symbol_name: &str,
        kind: NodeKind,
    ) -> Option<String> {
        let preferred_line = span.start[0];
        let symbol = symbol_name
            .strip_prefix("getter:")
            .or_else(|| symbol_name.strip_prefix("setter:"))
            .unwrap_or(symbol_name)
            .split('(')
            .next()
            .unwrap_or(symbol_name)
            .trim();

        let mut candidates = Vec::new();
        for candidate in [
            self.extract_exact_span_with_base(span, false),
            self.extract_exact_span_with_base(span, true),
            self.extract_full_lines_with_base(span, false),
            self.extract_full_lines_with_base(span, true),
            self.declaration_block_for_symbol(symbol, kind),
        ]
        .into_iter()
        .flatten()
        {
            if !candidate.trim().is_empty() && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }

        if let Some(best) = candidates
            .into_iter()
            .max_by_key(|candidate| Self::score_candidate(candidate, symbol, kind, preferred_line))
        {
            let best_score = Self::score_candidate(&best, symbol, kind, preferred_line);
            if best_score.0 > 0 || best_score.1 > 0 {
                return Some(trim_snippet_indentation(&best));
            }
        }

        self.declaration_line_for_symbol(symbol, kind)
            .map(|snippet| trim_snippet_indentation(&snippet))
    }

    #[allow(dead_code)]
    pub fn extract_snippet(&self, span: &Span, max_len: usize) -> Option<String> {
        let full = self.extract_full_snippet(span)?;

        if full.len() <= max_len {
            return Some(full);
        }

        let mut truncate_at = max_len;
        while !full.is_char_boundary(truncate_at) {
            truncate_at -= 1;
        }

        let truncated = &full[..truncate_at];
        match truncated.rfind('\n') {
            Some(pos) if pos > 0 => Some(truncated[..pos].to_string()),
            _ => Some(truncated.to_string()),
        }
    }
}

fn declaration_matches_symbol(line: &str, symbol: &str, kind: NodeKind) -> bool {
    if !line.contains(symbol) {
        return false;
    }

    match kind {
        NodeKind::Function => {
            line.contains("func ")
                || line.contains("init(")
                || line.contains("subscript")
                || line.contains("var ")
        }
        NodeKind::Property | NodeKind::Field | NodeKind::Constant => {
            line.contains("var ") || line.contains("let ")
        }
        NodeKind::Struct => line.contains("struct "),
        NodeKind::Trait => line.contains("trait "),
        NodeKind::Impl => line.contains("impl "),
        NodeKind::Enum => line.contains("enum "),
        NodeKind::Protocol => line.contains("protocol "),
        NodeKind::Extension => line.contains("extension "),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{LineIndex, trim_snippet_indentation};
    use grapha_core::graph::{NodeKind, Span};

    #[test]
    fn extract_snippet_truncates_single_line_at_utf8_boundary() {
        let source = "abc中def";
        let index = LineIndex::new(source);
        let span = Span {
            start: [0, 0],
            end: [0, 0],
        };

        assert_eq!(index.extract_snippet(&span, 4), Some("abc".to_string()));
    }

    #[test]
    fn extract_snippet_truncates_multiline_at_newline_before_utf8_cutoff() {
        let source = "alpha\n中文beta";
        let index = LineIndex::new(source);
        let span = Span {
            start: [0, 0],
            end: [1, 0],
        };

        assert_eq!(index.extract_snippet(&span, 8), Some("alpha".to_string()));
    }

    #[test]
    fn extract_full_snippet_uses_exact_columns() {
        let source = "let before = 1; fn hello() {\n    println!(\"hi\");\n} let after = 2;";
        let index = LineIndex::new(source);
        let span = Span {
            start: [0, 16],
            end: [2, 1],
        };

        assert_eq!(
            index.extract_full_snippet(&span),
            Some("fn hello() {\n    println!(\"hi\");\n}".to_string())
        );
    }

    #[test]
    fn extract_full_snippet_accepts_one_based_lines_as_fallback() {
        let source = "struct A {}\nfn greet() {\n    println!(\"hi\");\n}\n";
        let index = LineIndex::new(source);
        let span = Span {
            start: [2, 0],
            end: [4, 1],
        };

        assert_eq!(
            index.extract_full_snippet(&span),
            Some("fn greet() {\n    println!(\"hi\");\n}".to_string())
        );
    }

    #[test]
    fn extract_symbol_snippet_prefers_candidate_that_contains_symbol_name() {
        let source = "@Published private(set) var homeEffect: UserHomeDynamicInfo?\n@Published private(set) var hasInBlackList: Bool = false\n";
        let index = LineIndex::new(source);
        let span = Span {
            start: [1, 0],
            end: [1, 54],
        };

        assert_eq!(
            index.extract_symbol_snippet(&span, "homeEffect", NodeKind::Property),
            Some("@Published private(set) var homeEffect: UserHomeDynamicInfo?".to_string())
        );
    }

    #[test]
    fn extract_symbol_snippet_falls_back_to_matching_declaration_line() {
        let source = "@Published private(set) var remarkName: String = \"\"\n";
        let index = LineIndex::new(source);
        let span = Span {
            start: [35, 0],
            end: [35, 0],
        };

        assert_eq!(
            index.extract_symbol_snippet(&span, "remarkName", NodeKind::Property),
            Some("@Published private(set) var remarkName: String = \"\"".to_string())
        );
    }

    #[test]
    fn extract_symbol_snippet_recovers_full_function_block_from_source() {
        let source = "    @inline(__always) private func requestGetUser(\n        _ data: SingleUserRequest\n    ) async throws(RequestError) -> UserInfo {\n        try await request(\n            \"user/getUserInfoByUid/\\\\(data.id)\",\n            data: [\"attrs\": data.attrs.map(\\\\.rawValue).sorted()]\n        )\n    }\n";
        let index = LineIndex::new(source);
        let span = Span {
            start: [0, 4],
            end: [0, 39],
        };

        assert_eq!(
            index.extract_symbol_snippet(&span, "requestGetUser(_:)", NodeKind::Function),
            Some(
                "@inline(__always) private func requestGetUser(\n    _ data: SingleUserRequest\n) async throws(RequestError) -> UserInfo {\n    try await request(\n        \"user/getUserInfoByUid/\\\\(data.id)\",\n        data: [\"attrs\": data.attrs.map(\\\\.rawValue).sorted()]\n    )\n}"
                    .to_string()
            )
        );
    }

    #[test]
    fn trim_snippet_indentation_removes_shared_leading_spaces() {
        assert_eq!(
            trim_snippet_indentation("    func demo() {\n        work()\n    }"),
            "func demo() {\n    work()\n}".to_string()
        );
    }
}
