use std::path::Path;

use super::{ExtractionResult, LanguageExtractor};

pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn language(&self) -> &str {
        "rust"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn extract(&self, _source: &[u8], _file_path: &Path) -> anyhow::Result<ExtractionResult> {
        Ok(ExtractionResult::new())
    }
}
