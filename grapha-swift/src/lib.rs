mod treesitter;

use std::path::Path;

pub use treesitter::SwiftExtractor;

use grapha_core::{ExtractionResult, LanguageExtractor};

/// Extract Swift source code into a graph representation.
///
/// Waterfall: index-store → SwiftSyntax → tree-sitter
pub fn extract_swift(
    source: &[u8],
    file_path: &Path,
    _index_store_path: Option<&Path>,
) -> anyhow::Result<ExtractionResult> {
    let extractor = SwiftExtractor;
    extractor.extract(source, file_path)
}
