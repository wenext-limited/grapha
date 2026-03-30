mod bridge;
mod indexstore;
mod swiftsyntax;
mod treesitter;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use treesitter::SwiftExtractor;

use grapha_core::{ExtractionResult, LanguageExtractor};

static INDEX_STORE_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Auto-discover the Xcode index store for a project.
fn discover_index_store(project_path: &Path) -> Option<PathBuf> {
    // Check DerivedData
    let home = std::env::var("HOME").ok()?;
    let derived_data = Path::new(&home).join("Library/Developer/Xcode/DerivedData");

    if !derived_data.is_dir() {
        return None;
    }

    // Get project name from path
    let project_name = project_path.file_name()?.to_str()?;

    // Scan DerivedData for matching project
    for entry in std::fs::read_dir(&derived_data).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(project_name)
            || name_str.starts_with(&project_name.replace("-", ""))
        {
            let store = entry.path().join("Index.noindex/DataStore");
            if store.is_dir() {
                return Some(store);
            }
        }
    }
    None
}

/// Extract Swift source code with waterfall strategy:
/// 1. Xcode index store (confidence 1.0)
/// 2. SwiftSyntax bridge (confidence 0.9)
/// 3. tree-sitter-swift fallback (confidence 0.6-0.8)
pub fn extract_swift(
    source: &[u8],
    file_path: &Path,
    index_store_path: Option<&Path>,
) -> anyhow::Result<ExtractionResult> {
    // Use explicit path or auto-discover from DerivedData
    let discovered = INDEX_STORE_PATH.get_or_init(|| {
        discover_index_store(file_path.ancestors().nth(2).unwrap_or(file_path))
    });

    let effective_store = index_store_path.or(discovered.as_deref());

    if let Some(store_path) = effective_store {
        if let Some(result) = indexstore::extract_from_indexstore(file_path, store_path) {
            return Ok(result);
        }
    }

    if let Some(result) = swiftsyntax::extract_with_swiftsyntax(source, file_path) {
        return Ok(result);
    }

    let extractor = SwiftExtractor;
    extractor.extract(source, file_path)
}
