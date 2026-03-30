pub mod context;
pub mod entries;
pub mod impact;
pub mod reverse;
pub mod trace;

use serde::Serialize;

use grapha_core::graph::{Node, NodeKind};

/// Find a node by query — matches exact ID, exact name, or name prefix (for
/// USR-based names like `bootstrapGame(with:)` matching query `bootstrapGame`).
pub fn find_node<'a>(nodes: &'a [Node], query: &str) -> Option<&'a Node> {
    // 1. Exact match on ID or name
    if let Some(n) = nodes.iter().find(|n| n.id == query || n.name == query) {
        return Some(n);
    }
    // 2. Name starts with query (handles `bootstrapGame` matching `bootstrapGame(with:)`)
    let matches: Vec<_> = nodes
        .iter()
        .filter(|n| n.name.starts_with(query) || n.id.ends_with(query))
        .collect();
    if !matches.is_empty() {
        // Prefer exact name prefix over partial ID match
        return Some(matches[0]);
    }
    // 3. Case-insensitive contains on name
    let query_lower = query.to_lowercase();
    let ci_matches: Vec<_> = nodes
        .iter()
        .filter(|n| n.name.to_lowercase() == query_lower)
        .collect();
    if ci_matches.len() == 1 {
        return Some(ci_matches[0]);
    }
    // 4. File path component match (e.g., "WebGameRuntime.swift::bootstrapGame")
    if query.contains("::") {
        return nodes.iter().find(|n| n.id.ends_with(query));
    }
    None
}

#[derive(Debug, Serialize)]
pub struct ContextResult {
    pub symbol: SymbolInfo,
    pub callers: Vec<SymbolRef>,
    pub callees: Vec<SymbolRef>,
    pub implementors: Vec<SymbolRef>,
    pub implements: Vec<SymbolRef>,
    pub type_refs: Vec<SymbolRef>,
}

#[derive(Debug, Serialize)]
pub struct SymbolInfo {
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    pub file: String,
    pub span: [usize; 2],
}

#[derive(Debug, Serialize)]
pub struct SymbolRef {
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    pub file: String,
}
