use grapha_core::graph::{NodeKind, Span};

pub fn should_extract_snippet(kind: NodeKind) -> bool {
    !matches!(
        kind,
        NodeKind::Field
            | NodeKind::Variant
            | NodeKind::Property
            | NodeKind::Constant
            | NodeKind::View
            | NodeKind::Branch
    )
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
        Self { source, line_starts }
    }

    pub fn extract_snippet(&self, span: &Span, max_len: usize) -> Option<String> {
        let start_line = span.start[0];
        let end_line = span.end[0];

        if start_line >= self.line_starts.len() {
            return None;
        }

        let end_line = end_line.min(self.line_starts.len().saturating_sub(1));
        let byte_start = self.line_starts[start_line];

        // Find byte end: end of end_line (or end of source)
        let byte_end = if end_line + 1 < self.line_starts.len() {
            // Strip trailing newline
            self.line_starts[end_line + 1].saturating_sub(1)
        } else {
            self.source.len()
        };

        let slice = &self.source[byte_start..byte_end];
        // Trim trailing whitespace/newlines
        let slice = slice.trim_end();

        if slice.len() <= max_len {
            return Some(slice.to_string());
        }

        // Truncate at a clean line boundary within max_len
        let truncated = &slice[..max_len];
        match truncated.rfind('\n') {
            Some(pos) if pos > 0 => Some(truncated[..pos].to_string()),
            _ => Some(truncated.to_string()),
        }
    }
}

/// Convenience function for single-call usage (tests).
pub fn extract_snippet(source: &str, span: &Span, max_len: usize) -> Option<String> {
    LineIndex::new(source).extract_snippet(span, max_len)
}
