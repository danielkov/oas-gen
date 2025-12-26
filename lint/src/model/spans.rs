use rustc_hash::{FxHashMap, FxHashSet};

/// Byte span in the source document (end-exclusive)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteSpan {
    pub start: u32,
    pub end: u32,
}

impl ByteSpan {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Line and column position (0-based internally)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

impl LineCol {
    pub fn new(line: u32, col: u32) -> Self {
        Self { line, col }
    }
}

/// Range in line/column coordinates for IDE integration
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: LineCol,
    pub end: LineCol,
}

impl Range {
    pub fn new(start: LineCol, end: LineCol) -> Self {
        Self { start, end }
    }
}

/// Database mapping JSON Pointers to their byte spans in the source.
/// Tracks both key spans and value spans, as well as alias locations.
#[derive(Debug, Default)]
pub struct SpanDb {
    /// Spans for values at each JSON Pointer path
    pub value_spans: FxHashMap<String, ByteSpan>,
    /// Spans for keys at each JSON Pointer path
    pub key_spans: FxHashMap<String, ByteSpan>,
    /// Pointers that are YAML alias use-sites
    pub alias_ptrs: FxHashSet<String>,
}

impl SpanDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the value span for a pointer, falling back to nearest ancestor
    /// if the exact pointer is not found (Strategy A from design doc).
    pub fn resolve_value_span(&self, ptr: &str) -> Option<(ByteSpan, bool)> {
        // Try exact match first
        if let Some(&span) = self.value_spans.get(ptr) {
            let is_alias = self.alias_ptrs.contains(ptr);
            return Some((span, is_alias));
        }

        // Fall back to nearest ancestor
        let mut current = ptr.to_string();
        while let Some(last_slash) = current.rfind('/') {
            current.truncate(last_slash);
            if let Some(&span) = self.value_spans.get(&current) {
                let is_alias = self.alias_ptrs.contains(&current);
                return Some((span, is_alias));
            }
        }

        // Try root
        if let Some(&span) = self.value_spans.get("") {
            return Some((span, false));
        }

        None
    }
}

/// Line offset index for converting byte offsets to line/column
#[derive(Debug)]
pub struct LineIndex {
    /// Byte offset of the start of each line
    line_starts: Vec<u32>,
}

impl LineIndex {
    /// Build line index from source text
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, c) in source.char_indices() {
            if c == '\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self { line_starts }
    }

    /// Convert byte offset to line/column (0-based)
    pub fn line_col(&self, offset: u32) -> LineCol {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line];
        LineCol::new(line as u32, col)
    }

    /// Convert byte span to range
    pub fn range(&self, span: ByteSpan) -> Range {
        Range::new(self.line_col(span.start), self.line_col(span.end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index() {
        let source = "hello\nworld\nfoo";
        let index = LineIndex::new(source);

        assert_eq!(index.line_col(0), LineCol::new(0, 0)); // 'h'
        assert_eq!(index.line_col(5), LineCol::new(0, 5)); // '\n'
        assert_eq!(index.line_col(6), LineCol::new(1, 0)); // 'w'
        assert_eq!(index.line_col(12), LineCol::new(2, 0)); // 'f'
    }

    #[test]
    fn test_span_db_fallback() {
        let mut db = SpanDb::new();
        db.value_spans
            .insert("/paths".to_string(), ByteSpan::new(10, 20));
        db.value_spans
            .insert("/paths/~1pets".to_string(), ByteSpan::new(15, 18));

        // Exact match
        let (span, is_alias) = db.resolve_value_span("/paths/~1pets").unwrap();
        assert_eq!(span, ByteSpan::new(15, 18));
        assert!(!is_alias);

        // Fallback to parent
        let (span, _) = db.resolve_value_span("/paths/~1pets/get").unwrap();
        assert_eq!(span, ByteSpan::new(15, 18));

        // Fallback to grandparent
        let (span, _) = db
            .resolve_value_span("/paths/~1pets/get/operationId")
            .unwrap();
        assert_eq!(span, ByteSpan::new(15, 18));
    }
}
