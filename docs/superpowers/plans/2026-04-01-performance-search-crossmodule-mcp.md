# Grapha v2: Performance, Search, Cross-Module, MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make grapha faster (batch SQLite, parallel init), add advanced search (filters, fuzzy, context mode), source snippets, output field customization, cross-module graph analysis, MCP server, and nodus packaging.

**Architecture:** Performance-first approach — optimize the pipeline (batch inserts, parallel plugin init), then add the snippet field that search context mode depends on, then layer on search filters, output customization, cross-module support, MCP server, and finally nodus packaging.

**Tech Stack:** Rust, rusqlite 0.34 (bundled), tantivy 0.25, clap 4, axum 0.8, tokio, serde, toml 0.8, rayon

---

## Phase 1: Pipeline Performance

### Task 1: Batch SQLite Inserts

**Files:**
- Modify: `grapha/src/store/sqlite.rs:136-218` (insert_nodes, insert_edges)
- Test: `grapha/tests/store_batch_test.rs` (new)

- [ ] **Step 1: Write failing test for batch node insert**

```rust
// grapha/tests/store_batch_test.rs
use grapha_core::graph::*;
use std::collections::HashMap;
use std::path::PathBuf;

fn make_test_nodes(count: usize) -> Vec<Node> {
    (0..count)
        .map(|i| Node {
            id: format!("node_{i}"),
            kind: NodeKind::Function,
            name: format!("func_{i}"),
            file: PathBuf::from(format!("src/mod_{i}.rs")),
            span: Span {
                start: [i, 0],
                end: [i + 10, 0],
            },
            visibility: Visibility::Public,
            metadata: HashMap::new(),
            role: None,
            signature: Some(format!("fn func_{i}()")),
            doc_comment: None,
            module: Some("test".to_string()),
            snippet: None,
        })
        .collect()
}

#[test]
fn batch_insert_nodes_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = grapha::store::sqlite::SqliteStore::new(db_path);
    let nodes = make_test_nodes(1500);
    let graph = Graph {
        version: "0.1.0".to_string(),
        nodes,
        edges: vec![],
    };
    store.save(&graph).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.nodes.len(), 1500);
    for (orig, loaded) in graph.nodes.iter().zip(loaded.nodes.iter()) {
        assert_eq!(orig.id, loaded.id);
        assert_eq!(orig.name, loaded.name);
        assert_eq!(orig.signature, loaded.signature);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p grapha --test store_batch_test -- batch_insert_nodes_round_trips`
Expected: FAIL (snippet field doesn't exist yet, test file doesn't exist)

- [ ] **Step 3: Add snippet field to Node (prerequisite)**

This is needed before batch insert changes. See Task 5 for the full snippet implementation — here we only add the field.

In `grapha-core/src/graph.rs`, add after the `module` field (line 105):

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
```

Update all `Node` construction sites across the codebase to include `snippet: None`. Use `grep -r "Node {" --include="*.rs"` to find them all.

- [ ] **Step 4: Add snippet column to SQLite schema**

In `grapha/src/store/sqlite.rs`, update both schema definitions (lines 46-48 in `create_tables` and lines 234-248 in `save_full`):

Add after the `module` column:
```sql
snippet TEXT
```

Update `STORE_SCHEMA_VERSION` from `"3"` to `"4"` (line 11).

- [ ] **Step 5: Rewrite insert_nodes with batch VALUES**

Replace `insert_nodes` (lines 136-180) with:

```rust
fn insert_nodes(
    tx: &rusqlite::Transaction<'_>,
    nodes: &[Node],
    replace: bool,
) -> anyhow::Result<()> {
    if nodes.is_empty() {
        return Ok(());
    }
    let verb = if replace { "INSERT OR REPLACE" } else { "INSERT" };
    let columns = "(id, kind, name, file,
        span_start_line, span_start_col, span_end_line, span_end_col,
        visibility, metadata, role, signature, doc_comment, module, snippet)";

    let batch_size = 500;
    let empty_meta = "{}";

    // Pre-serialize all metadata outside the insert loop
    let serialized: Vec<_> = nodes
        .iter()
        .map(|node| {
            let role_json: Option<String> =
                node.role.as_ref().map(serde_json::to_string).transpose()?;
            let meta_str = if node.metadata.is_empty() {
                empty_meta.to_string()
            } else {
                serde_json::to_string(&node.metadata)?
            };
            Ok((role_json, meta_str))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    for chunk_start in (0..nodes.len()).step_by(batch_size) {
        let chunk_end = (chunk_start + batch_size).min(nodes.len());
        let chunk = &nodes[chunk_start..chunk_end];
        let chunk_ser = &serialized[chunk_start..chunk_end];
        let placeholders: Vec<String> = (0..chunk.len())
            .map(|i| {
                let base = i * 15;
                format!(
                    "(?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{})",
                    base + 1, base + 2, base + 3, base + 4, base + 5,
                    base + 6, base + 7, base + 8, base + 9, base + 10,
                    base + 11, base + 12, base + 13, base + 14, base + 15,
                )
            })
            .collect();
        let sql = format!(
            "{verb} INTO nodes {columns} VALUES {}",
            placeholders.join(", ")
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::with_capacity(chunk.len() * 15);
        for (node, (role_json, meta_str)) in chunk.iter().zip(chunk_ser.iter()) {
            params.push(Box::new(node.id.clone()));
            params.push(Box::new(node_kind_str(&node.kind).to_string()));
            params.push(Box::new(node.name.clone()));
            params.push(Box::new(node.file.to_string_lossy().to_string()));
            params.push(Box::new(node.span.start[0] as i64));
            params.push(Box::new(node.span.start[1] as i64));
            params.push(Box::new(node.span.end[0] as i64));
            params.push(Box::new(node.span.end[1] as i64));
            params.push(Box::new(visibility_str(&node.visibility).to_string()));
            params.push(Box::new(meta_str.clone()));
            params.push(Box::new(role_json.clone()));
            params.push(Box::new(node.signature.clone()));
            params.push(Box::new(node.doc_comment.clone()));
            params.push(Box::new(node.module.clone()));
            params.push(Box::new(node.snippet.clone()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}
```

- [ ] **Step 6: Rewrite insert_edges with batch VALUES**

Replace `insert_edges` (lines 182-218) with the same batching pattern — 500 edges per statement, 10 columns each. Pre-serialize direction and provenance outside the loop.

```rust
fn insert_edges<'a>(
    tx: &rusqlite::Transaction<'_>,
    edges: impl Iterator<Item = (String, &'a Edge)>,
    replace: bool,
) -> anyhow::Result<()> {
    let verb = if replace { "INSERT OR REPLACE" } else { "INSERT" };
    let columns = "(edge_id, source, target, kind, confidence,
        direction, operation, condition, async_boundary, provenance)";

    let batch_size = 500;
    let collected: Vec<_> = edges.collect();
    if collected.is_empty() {
        return Ok(());
    }

    // Pre-serialize
    let serialized: Vec<_> = collected
        .iter()
        .map(|(_, edge)| {
            let direction_str: Option<String> =
                edge.direction.as_ref().map(enum_to_str).transpose()?;
            let provenance = serde_json::to_string(&edge.provenance)?;
            Ok((direction_str, provenance))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    for chunk_start in (0..collected.len()).step_by(batch_size) {
        let chunk_end = (chunk_start + batch_size).min(collected.len());
        let chunk = &collected[chunk_start..chunk_end];
        let chunk_ser = &serialized[chunk_start..chunk_end];
        let placeholders: Vec<String> = (0..chunk.len())
            .map(|i| {
                let base = i * 10;
                format!(
                    "(?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, ?{})",
                    base + 1, base + 2, base + 3, base + 4, base + 5,
                    base + 6, base + 7, base + 8, base + 9, base + 10,
                )
            })
            .collect();
        let sql = format!(
            "{verb} INTO edges {columns} VALUES {}",
            placeholders.join(", ")
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::with_capacity(chunk.len() * 10);
        for ((edge_id, edge), (direction_str, provenance)) in chunk.iter().zip(chunk_ser.iter()) {
            let async_boundary_int: Option<i64> =
                edge.async_boundary.map(|b| if b { 1 } else { 0 });
            params.push(Box::new(edge_id.clone()));
            params.push(Box::new(edge.source.clone()));
            params.push(Box::new(edge.target.clone()));
            params.push(Box::new(edge_kind_str(&edge.kind).to_string()));
            params.push(Box::new(edge.confidence));
            params.push(Box::new(direction_str.clone()));
            params.push(Box::new(edge.operation.clone()));
            params.push(Box::new(edge.condition.clone()));
            params.push(Box::new(async_boundary_int));
            params.push(Box::new(provenance.clone()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}
```

- [ ] **Step 7: Update load function for snippet column**

In `grapha/src/store/sqlite.rs`, update the `load` function's node query and deserialization to read the new `snippet` column. Add `snippet TEXT` to the SELECT and populate `node.snippet`.

- [ ] **Step 8: Add PRAGMA optimize after commit**

In `save_full` (after line 276) and `save_incremental` (after line 402), add:

```rust
conn.execute_batch("PRAGMA optimize;")?;
```

- [ ] **Step 9: Run the batch insert test**

Run: `cargo test -p grapha --test store_batch_test -- batch_insert_nodes_round_trips`
Expected: PASS

- [ ] **Step 10: Run full test suite**

Run: `cargo test`
Expected: All tests pass (may need to update Node construction sites with `snippet: None`)

- [ ] **Step 11: Commit**

```bash
git add grapha-core/src/graph.rs grapha/src/store/sqlite.rs grapha/tests/store_batch_test.rs
git commit -m "perf(store): batch SQLite inserts and add snippet field to Node"
```

---

### Task 2: Parallel Plugin Initialization

**Files:**
- Modify: `grapha/src/main.rs:253-272` (run_pipeline)
- Modify: `grapha/src/config.rs:5-28` (add swift config)
- Test: `grapha/tests/config_test.rs` (new)

- [ ] **Step 1: Write failing test for swift config parsing**

```rust
// grapha/tests/config_test.rs
#[test]
fn parse_swift_index_store_config() {
    let toml_str = r#"
[swift]
index_store = false

[[classifiers]]
pattern = "URLSession"
terminal = "network"
direction = "read"
operation = "HTTP_GET"
"#;
    let config: grapha::config::GraphaConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.swift.index_store, false);
    assert_eq!(config.classifiers.len(), 1);
}

#[test]
fn default_swift_config_enables_index_store() {
    let config: grapha::config::GraphaConfig = toml::from_str("").unwrap();
    assert_eq!(config.swift.index_store, true);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p grapha --test config_test`
Expected: FAIL (no `swift` field on GraphaConfig)

- [ ] **Step 3: Add SwiftConfig to GraphaConfig**

In `grapha/src/config.rs`, add:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SwiftConfig {
    #[serde(default = "default_true")]
    pub index_store: bool,
}

impl Default for SwiftConfig {
    fn default() -> Self {
        Self { index_store: true }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GraphaConfig {
    #[serde(default)]
    pub swift: SwiftConfig,
    #[serde(default)]
    pub classifiers: Vec<ClassifierRule>,
}
```

- [ ] **Step 4: Run config tests**

Run: `cargo test -p grapha --test config_test`
Expected: PASS

- [ ] **Step 5: Parallelize plugin init with file discovery**

In `grapha/src/main.rs`, modify `run_pipeline` (lines 253-272). Replace the sequential flow with:

```rust
fn run_pipeline(path: &Path, verbose: bool) -> anyhow::Result<grapha_core::graph::Graph> {
    let t = Instant::now();
    let registry = builtin_registry()?;
    let project_context = grapha_core::project_context(path);
    let cfg = config::load_config(path);

    // Run file discovery and plugin init concurrently
    let (files, plugin_result) = std::thread::scope(|scope| {
        let files_handle = scope.spawn(|| {
            grapha_core::pipeline::discover_files(path, &registry)
                .context("failed to discover files")
        });
        let plugin_handle = scope.spawn(|| {
            grapha_core::prepare_plugins(&registry, &project_context)
        });
        let files = files_handle.join().expect("discover thread panicked")?;
        let plugin = plugin_handle.join().expect("plugin thread panicked")?;
        Ok::<_, anyhow::Error>((files, plugin))
    })?;
    // plugin_result is () on success

    if verbose {
        progress::done(&format!("discovered {} files", files.len()), t);
    }

    let t_idx = Instant::now();
    if let Some(store) = grapha_swift::index_store_path()
        && verbose
    {
        progress::done(&format!("index store: {}", store.display()), t_idx);
    }

    let module_map = grapha_core::discover_modules(&registry, &project_context)?;
    // ... rest of pipeline unchanged ...
```

- [ ] **Step 6: Pass index_store config to Swift plugin**

This requires threading the config through to `grapha_swift`. For now, if `cfg.swift.index_store` is false, set an environment variable before plugin init that the Swift plugin checks:

```rust
if !cfg.swift.index_store {
    std::env::set_var("GRAPHA_SKIP_INDEX_STORE", "1");
}
```

Then in `grapha-swift/src/lib.rs`, check this env var in `prepare_project` and `extract_swift` to skip index store usage. This avoids changing the plugin trait signature.

- [ ] **Step 7: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 8: Commit**

```bash
git add grapha/src/main.rs grapha/src/config.rs grapha/tests/config_test.rs
git commit -m "perf(pipeline): parallelize plugin init with file discovery, add swift.index_store config"
```

---

### Task 3: Reuse GraphDelta for Search Sync

**Files:**
- Modify: `grapha/src/main.rs:623-664` (handle_index thread scope)
- Modify: `grapha/src/search.rs:109-160` (sync_index — add delta param)
- Test: existing search tests

- [ ] **Step 1: Add delta parameter to sync_index**

In `grapha/src/search.rs`, change `sync_index` signature to accept an optional pre-computed delta:

```rust
pub fn sync_index(
    previous: Option<&Graph>,
    graph: &Graph,
    index_path: &Path,
    force_full_rebuild: bool,
    precomputed_delta: Option<&GraphDelta>,
) -> Result<SearchSyncStats> {
```

Inside the function, replace `let delta = GraphDelta::between(previous_graph, graph);` with:

```rust
let owned_delta;
let delta = match precomputed_delta {
    Some(d) => d,
    None => {
        owned_delta = GraphDelta::between(previous_graph, graph);
        &owned_delta
    }
};
```

- [ ] **Step 2: Update handle_index to compute delta once and share**

In `grapha/src/main.rs` `handle_index`, compute the delta before the thread scope and pass it to both save and search threads:

```rust
let delta = if full_rebuild || previous_graph.is_none() {
    None
} else {
    Some(GraphDelta::between(previous_graph.as_ref().unwrap(), &graph))
};

let save_result = std::thread::scope(|scope| {
    let save_handle = scope.spawn(|| {
        // ... use delta.as_ref() for store save_incremental ...
    });
    let search_handle = scope.spawn(|| {
        let t = Instant::now();
        let stats = search::sync_index(
            previous_graph.as_ref(),
            &graph,
            &search_index_path,
            full_rebuild,
            delta.as_ref(),
        )?;
        Ok::<_, anyhow::Error>((t.elapsed(), stats))
    });
    // ...
});
```

- [ ] **Step 3: Update all other sync_index call sites**

Search for `sync_index(` across the codebase and add the new `None` parameter where no precomputed delta is available.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add grapha/src/main.rs grapha/src/search.rs
git commit -m "perf(search): reuse GraphDelta between store save and search sync"
```

---

## Phase 2: Source Snippets

### Task 4: Snippet Extraction During Pipeline

**Files:**
- Modify: `grapha/src/main.rs:286-320` (par_iter extraction loop)
- Create: `grapha/src/snippet.rs`
- Test: `grapha/tests/snippet_test.rs` (new)

Note: The `snippet` field on `Node` and the SQLite column were already added in Task 1.

- [ ] **Step 1: Write failing test for snippet extraction**

```rust
// grapha/tests/snippet_test.rs
#[test]
fn extract_snippet_from_source() {
    let source = "line 0\nline 1\nfn foo() {\n    bar();\n    baz();\n}\nline 6\n";
    // span: start line 2, end line 5
    let span = grapha_core::graph::Span {
        start: [2, 0],
        end: [5, 1],
    };
    let snippet = grapha::snippet::extract_snippet(source, &span, 600);
    assert_eq!(snippet, Some("fn foo() {\n    bar();\n    baz();\n}".to_string()));
}

#[test]
fn snippet_truncates_at_line_boundary() {
    let long_lines: String = (0..100).map(|i| format!("line {i} with some content here\n")).collect();
    let span = grapha_core::graph::Span {
        start: [0, 0],
        end: [99, 0],
    };
    let snippet = grapha::snippet::extract_snippet(&long_lines, &span, 100);
    let result = snippet.unwrap();
    assert!(result.len() <= 100);
    assert!(result.ends_with('\n') || !result.contains('\n') || result.len() <= 100);
}

#[test]
fn skip_snippet_for_leaf_kinds() {
    use grapha_core::graph::NodeKind;
    assert!(grapha::snippet::should_extract_snippet(NodeKind::Function));
    assert!(grapha::snippet::should_extract_snippet(NodeKind::Struct));
    assert!(!grapha::snippet::should_extract_snippet(NodeKind::Field));
    assert!(!grapha::snippet::should_extract_snippet(NodeKind::Variant));
    assert!(!grapha::snippet::should_extract_snippet(NodeKind::View));
    assert!(!grapha::snippet::should_extract_snippet(NodeKind::Branch));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p grapha --test snippet_test`
Expected: FAIL (module doesn't exist)

- [ ] **Step 3: Create snippet.rs**

```rust
// grapha/src/snippet.rs
use grapha_core::graph::{NodeKind, Span};

const DEFAULT_MAX_LEN: usize = 600;

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

pub fn extract_snippet(source: &str, span: &Span, max_len: usize) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let start_line = span.start[0];
    let end_line = span.end[0];

    if start_line >= lines.len() {
        return None;
    }

    let end_line = end_line.min(lines.len().saturating_sub(1));
    let span_lines = &lines[start_line..=end_line];
    let full = span_lines.join("\n");

    if full.len() <= max_len {
        return Some(full);
    }

    // Truncate at a clean line boundary
    let mut truncated = String::new();
    for line in span_lines {
        if truncated.len() + line.len() + 1 > max_len {
            break;
        }
        if !truncated.is_empty() {
            truncated.push('\n');
        }
        truncated.push_str(line);
    }

    if truncated.is_empty() {
        // Single very long line — hard truncate
        Some(full[..max_len].to_string())
    } else {
        Some(truncated)
    }
}
```

Add `pub mod snippet;` to `grapha/src/main.rs` (after line 13).

- [ ] **Step 4: Run snippet tests**

Run: `cargo test -p grapha --test snippet_test`
Expected: PASS

- [ ] **Step 5: Wire snippet extraction into the pipeline**

In `grapha/src/main.rs`, modify the `par_iter` extraction loop (lines 286-320). After extraction succeeds, populate snippets:

```rust
match extraction_result {
    Ok(mut result) => {
        // Populate snippets from source for eligible node kinds
        for node in &mut result.nodes {
            if snippet::should_extract_snippet(node.kind) {
                node.snippet = snippet::extract_snippet(
                    &String::from_utf8_lossy(&source),
                    &node.span,
                    600,
                );
            }
        }
        Some(result)
    }
    Err(e) => { /* ... unchanged ... */ }
}
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
git add grapha/src/snippet.rs grapha/src/main.rs
git commit -m "feat(core): extract source snippets during pipeline for token-efficient browsing"
```

---

## Phase 3: Advanced Search

### Task 5: Expand Tantivy Schema and Add Filters

**Files:**
- Modify: `grapha/src/search.rs:14-72` (SearchResult, SearchFields, schema)
- Modify: `grapha/src/main.rs:119-131` (SymbolCommands::Search CLI args)
- Test: `grapha/tests/search_filter_test.rs` (new)

- [ ] **Step 1: Write failing test for filtered search**

```rust
// grapha/tests/search_filter_test.rs
use grapha_core::graph::*;
use std::collections::HashMap;
use std::path::PathBuf;

fn build_test_index() -> (tempfile::TempDir, tantivy::Index) {
    let dir = tempfile::tempdir().unwrap();
    let index_path = dir.path().join("search_index");
    let graph = Graph {
        version: "test".to_string(),
        nodes: vec![
            Node {
                id: "mod_a::MyStruct".to_string(),
                kind: NodeKind::Struct,
                name: "MyStruct".to_string(),
                file: PathBuf::from("src/model.rs"),
                span: Span { start: [1, 0], end: [10, 0] },
                visibility: Visibility::Public,
                metadata: HashMap::new(),
                role: None,
                signature: None,
                doc_comment: None,
                module: Some("mod_a".to_string()),
                snippet: None,
            },
            Node {
                id: "mod_a::my_func".to_string(),
                kind: NodeKind::Function,
                name: "my_func".to_string(),
                file: PathBuf::from("src/handler.rs"),
                span: Span { start: [5, 0], end: [20, 0] },
                visibility: Visibility::Public,
                metadata: HashMap::new(),
                role: Some(NodeRole::EntryPoint),
                signature: Some("fn my_func()".to_string()),
                doc_comment: None,
                module: Some("mod_a".to_string()),
                snippet: Some("fn my_func() {\n    todo!()\n}".to_string()),
            },
            Node {
                id: "mod_b::my_func".to_string(),
                kind: NodeKind::Function,
                name: "my_func".to_string(),
                file: PathBuf::from("src/other.rs"),
                span: Span { start: [1, 0], end: [5, 0] },
                visibility: Visibility::Private,
                metadata: HashMap::new(),
                role: None,
                signature: None,
                doc_comment: None,
                module: Some("mod_b".to_string()),
                snippet: None,
            },
        ],
        edges: vec![],
    };
    grapha::search::sync_index(None, &graph, &index_path, true, None).unwrap();
    let index = tantivy::Index::open_in_dir(&index_path).unwrap();
    (dir, index)
}

#[test]
fn search_with_kind_filter() {
    let (_dir, index) = build_test_index();
    let opts = grapha::search::SearchOptions {
        kind: Some("function".to_string()),
        ..Default::default()
    };
    let results = grapha::search::search_filtered(&index, "my", 20, &opts).unwrap();
    assert!(results.iter().all(|r| r.kind == "function"));
    assert_eq!(results.len(), 2);
}

#[test]
fn search_with_module_filter() {
    let (_dir, index) = build_test_index();
    let opts = grapha::search::SearchOptions {
        module: Some("mod_a".to_string()),
        ..Default::default()
    };
    let results = grapha::search::search_filtered(&index, "my", 20, &opts).unwrap();
    assert!(results.iter().all(|r| r.module.as_deref() == Some("mod_a")));
}

#[test]
fn search_with_role_filter() {
    let (_dir, index) = build_test_index();
    let opts = grapha::search::SearchOptions {
        role: Some("entry_point".to_string()),
        ..Default::default()
    };
    let results = grapha::search::search_filtered(&index, "my", 20, &opts).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "my_func");
    assert_eq!(results[0].module, Some("mod_a".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p grapha --test search_filter_test`
Expected: FAIL

- [ ] **Step 3: Expand SearchFields and schema**

In `grapha/src/search.rs`, update the schema:

```rust
#[derive(Clone, Copy)]
struct SearchFields {
    id: tantivy::schema::Field,
    name: tantivy::schema::Field,
    kind: tantivy::schema::Field,
    file: tantivy::schema::Field,
    module: tantivy::schema::Field,
    visibility: tantivy::schema::Field,
    role: tantivy::schema::Field,
}

fn schema() -> (Schema, SearchFields) {
    let mut schema_builder = Schema::builder();
    let id = schema_builder.add_text_field("id", STRING | STORED);
    let name = schema_builder.add_text_field("name", TEXT | STORED);
    let kind = schema_builder.add_text_field("kind", STRING | STORED);
    let file = schema_builder.add_text_field("file", TEXT | STORED);
    let module = schema_builder.add_text_field("module", STRING | STORED);
    let visibility = schema_builder.add_text_field("visibility", STRING | STORED);
    let role = schema_builder.add_text_field("role", STRING | STORED);
    (
        schema_builder.build(),
        SearchFields { id, name, kind, file, module, visibility, role },
    )
}
```

- [ ] **Step 4: Update SearchResult and add SearchOptions**

```rust
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Default)]
pub struct SearchOptions {
    pub kind: Option<String>,
    pub module: Option<String>,
    pub file_glob: Option<String>,
    pub role: Option<String>,
    pub fuzzy: bool,
}
```

- [ ] **Step 5: Implement search_filtered**

```rust
pub fn search_filtered(
    index: &Index,
    query_str: &str,
    limit: usize,
    options: &SearchOptions,
) -> Result<Vec<SearchResult>> {
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    let searcher = reader.searcher();
    let fields = resolve_fields(index)?;

    // Build text query
    let text_query: Box<dyn tantivy::query::Query> = if options.fuzzy {
        let term = Term::from_field_text(fields.name, query_str);
        Box::new(tantivy::query::FuzzyTermQuery::new(term, 2, true))
    } else {
        let query_parser = QueryParser::for_index(index, vec![fields.name, fields.file]);
        Box::new(query_parser.parse_query(query_str)?)
    };

    // Compose boolean query with filters
    let mut must_clauses: Vec<(tantivy::query::Occur, Box<dyn tantivy::query::Query>)> = vec![
        (tantivy::query::Occur::Must, text_query),
    ];

    if let Some(ref kind) = options.kind {
        must_clauses.push((
            tantivy::query::Occur::Must,
            Box::new(tantivy::query::TermQuery::new(
                Term::from_field_text(fields.kind, kind),
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }
    if let Some(ref module) = options.module {
        must_clauses.push((
            tantivy::query::Occur::Must,
            Box::new(tantivy::query::TermQuery::new(
                Term::from_field_text(fields.module, module),
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }
    if let Some(ref role) = options.role {
        must_clauses.push((
            tantivy::query::Occur::Must,
            Box::new(tantivy::query::TermQuery::new(
                Term::from_field_text(fields.role, role),
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    let combined = tantivy::query::BooleanQuery::from(must_clauses);
    let top_docs = searcher.search(&combined, &TopDocs::with_limit(limit))?;

    let mut results = Vec::new();
    for (score, doc_address) in top_docs {
        let doc: TantivyDocument = searcher.doc(doc_address)?;
        let get = |f: tantivy::schema::Field| -> String {
            doc.get_first(f)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let module_val = get(fields.module);
        let role_val = get(fields.role);
        results.push(SearchResult {
            id: get(fields.id),
            name: get(fields.name),
            kind: get(fields.kind),
            file: get(fields.file),
            score,
            module: if module_val.is_empty() { None } else { Some(module_val) },
            role: if role_val.is_empty() { None } else { Some(role_val) },
        });
    }
    Ok(results)
}
```

- [ ] **Step 6: Update node_document to populate new fields**

Update the `node_document` helper that creates Tantivy documents to include module, visibility, and role fields from the Node.

- [ ] **Step 7: Keep the old `search()` function as a wrapper**

```rust
pub fn search(index: &Index, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
    search_filtered(index, query_str, limit, &SearchOptions::default())
}
```

- [ ] **Step 8: Run filter tests**

Run: `cargo test -p grapha --test search_filter_test`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add grapha/src/search.rs grapha/tests/search_filter_test.rs
git commit -m "feat(search): add kind/module/role filters and fuzzy search"
```

---

### Task 6: Search CLI Flags and Elapsed Timing

**Files:**
- Modify: `grapha/src/main.rs:119-131` (SymbolCommands::Search args)
- Modify: `grapha/src/main.rs:711-716` (handle search command)

- [ ] **Step 1: Add CLI args to Search subcommand**

```rust
Search {
    /// Search query
    query: String,
    /// Max results
    #[arg(long, default_value = "20")]
    limit: usize,
    /// Project directory
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
    /// Filter by symbol kind (function, struct, enum, trait, etc.)
    #[arg(long)]
    kind: Option<String>,
    /// Filter by module name
    #[arg(long)]
    module: Option<String>,
    /// Filter by file path glob
    #[arg(long)]
    file: Option<String>,
    /// Filter by role (entry_point, terminal, internal)
    #[arg(long)]
    role: Option<String>,
    /// Enable fuzzy matching (tolerates typos)
    #[arg(long)]
    fuzzy: bool,
    /// Include source snippet and relationships in results
    #[arg(long)]
    context: bool,
},
```

- [ ] **Step 2: Update handle_symbol_command for new args + timing**

```rust
SymbolCommands::Search {
    query, limit, path, kind, module, file, role, fuzzy, context,
} => {
    let index = open_search_index(&path)?;
    let options = search::SearchOptions {
        kind,
        module,
        file_glob: file,
        role,
        fuzzy,
    };
    let t = Instant::now();
    let results = search::search_filtered(&index, &query, limit, &options)?;
    let elapsed = t.elapsed();

    if context {
        // Load graph for relationship data
        let graph = load_graph(&path)?;
        let enriched = search::enrich_with_context(&results, &graph);
        print_json(&enriched)?;
    } else {
        print_json(&results)?;
    }

    eprintln!(
        "\n  {} results in {:.1}ms",
        results.len(),
        elapsed.as_secs_f64() * 1000.0,
    );
    Ok(())
}
```

- [ ] **Step 3: Implement enrich_with_context**

In `grapha/src/search.rs`, add a function that takes search results and a graph, then for each result finds its direct callers/callees/type_refs and snippet:

```rust
#[derive(Debug, Serialize)]
pub struct EnrichedSearchResult {
    #[serde(flatten)]
    pub base: SearchResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub called_by: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub type_refs: Vec<String>,
}

pub fn enrich_with_context(results: &[SearchResult], graph: &Graph) -> Vec<EnrichedSearchResult> {
    results
        .iter()
        .map(|r| {
            let node = graph.nodes.iter().find(|n| n.id == r.id);
            let snippet = node.and_then(|n| n.snippet.clone());

            let calls: Vec<String> = graph
                .edges
                .iter()
                .filter(|e| e.source == r.id && e.kind == EdgeKind::Calls)
                .map(|e| e.target.clone())
                .collect();

            let called_by: Vec<String> = graph
                .edges
                .iter()
                .filter(|e| e.target == r.id && e.kind == EdgeKind::Calls)
                .map(|e| e.source.clone())
                .collect();

            let type_refs: Vec<String> = graph
                .edges
                .iter()
                .filter(|e| e.source == r.id && e.kind == EdgeKind::TypeRef)
                .map(|e| e.target.clone())
                .collect();

            EnrichedSearchResult {
                base: SearchResult {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    kind: r.kind.clone(),
                    file: r.file.clone(),
                    score: r.score,
                    module: r.module.clone(),
                    role: r.role.clone(),
                },
                snippet,
                calls,
                called_by,
                type_refs,
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add grapha/src/main.rs grapha/src/search.rs
git commit -m "feat(search): add CLI filters, fuzzy, context mode, and elapsed timing"
```

---

### Task 7: Fix Web API Search

**Files:**
- Modify: `grapha/src/serve.rs:13-15` (AppState)
- Modify: `grapha/src/serve/api.rs:83-104` (get_search)

- [ ] **Step 1: Add search index to AppState**

```rust
pub struct AppState {
    pub graph: Graph,
    pub search_index: tantivy::Index,
}
```

Update `serve::run()` to accept and store the search index. Update the caller in `main.rs` that calls `serve::run()` to pass the index.

- [ ] **Step 2: Replace naive search with Tantivy**

In `grapha/src/serve/api.rs`, replace `get_search`:

```rust
pub async fn get_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Json<serde_json::Value> {
    let options = crate::search::SearchOptions::default();
    let results = match crate::search::search_filtered(
        &state.search_index,
        &params.q,
        params.limit,
        &options,
    ) {
        Ok(results) => results,
        Err(_) => vec![],
    };
    Json(serde_json::json!({ "results": results, "total": results.len() }))
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add grapha/src/serve.rs grapha/src/serve/api.rs
git commit -m "fix(serve): replace naive substring search with Tantivy index"
```

---

## Phase 4: Output Customization

### Task 8: FieldSet and --fields Flag

**Files:**
- Create: `grapha/src/fields.rs`
- Modify: `grapha/src/render.rs:13-30` (RenderOptions)
- Modify: `grapha/src/config.rs` (add output config)
- Modify: `grapha/src/main.rs` (add --fields to commands)
- Modify: `grapha/src/query.rs:197-212` (SymbolRef, SymbolInfo)
- Test: `grapha/tests/fields_test.rs` (new)

- [ ] **Step 1: Write failing test for FieldSet parsing**

```rust
// grapha/tests/fields_test.rs
#[test]
fn parse_fields_from_string() {
    let fs = grapha::fields::FieldSet::parse("file,module,signature");
    assert!(fs.file);
    assert!(fs.module);
    assert!(fs.signature);
    assert!(!fs.id);
    assert!(!fs.snippet);
}

#[test]
fn parse_all_fields() {
    let fs = grapha::fields::FieldSet::parse("all");
    assert!(fs.file && fs.id && fs.module && fs.span && fs.snippet
        && fs.visibility && fs.signature && fs.role);
}

#[test]
fn parse_none_fields() {
    let fs = grapha::fields::FieldSet::parse("none");
    assert!(!fs.file && !fs.id && !fs.module && !fs.span && !fs.snippet
        && !fs.visibility && !fs.signature && !fs.role);
}

#[test]
fn default_fields() {
    let fs = grapha::fields::FieldSet::default();
    assert!(fs.file);
    assert!(!fs.id && !fs.module && !fs.span && !fs.snippet
        && !fs.visibility && !fs.signature && !fs.role);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p grapha --test fields_test`
Expected: FAIL

- [ ] **Step 3: Create fields.rs**

```rust
// grapha/src/fields.rs
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSet {
    pub file: bool,
    pub id: bool,
    pub module: bool,
    pub span: bool,
    pub snippet: bool,
    pub visibility: bool,
    pub signature: bool,
    pub role: bool,
}

impl Default for FieldSet {
    fn default() -> Self {
        Self {
            file: true,
            id: false,
            module: false,
            span: false,
            snippet: false,
            visibility: false,
            signature: false,
            role: false,
        }
    }
}

impl FieldSet {
    pub fn parse(input: &str) -> Self {
        match input.trim() {
            "all" => Self {
                file: true, id: true, module: true, span: true,
                snippet: true, visibility: true, signature: true, role: true,
            },
            "none" => Self {
                file: false, id: false, module: false, span: false,
                snippet: false, visibility: false, signature: false, role: false,
            },
            s => {
                let mut fs = Self {
                    file: false, id: false, module: false, span: false,
                    snippet: false, visibility: false, signature: false, role: false,
                };
                for field in s.split(',') {
                    match field.trim() {
                        "file" => fs.file = true,
                        "id" => fs.id = true,
                        "module" => fs.module = true,
                        "span" => fs.span = true,
                        "snippet" => fs.snippet = true,
                        "visibility" => fs.visibility = true,
                        "signature" => fs.signature = true,
                        "role" => fs.role = true,
                        _ => {} // ignore unknown fields
                    }
                }
                fs
            }
        }
    }

    pub fn from_config(fields: &[String]) -> Self {
        Self::parse(&fields.join(","))
    }
}
```

Add `pub mod fields;` to `grapha/src/main.rs`.

- [ ] **Step 4: Run field tests**

Run: `cargo test -p grapha --test fields_test`
Expected: PASS

- [ ] **Step 5: Add output config to GraphaConfig**

In `grapha/src/config.rs`:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct OutputConfig {
    #[serde(default)]
    pub default_fields: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GraphaConfig {
    #[serde(default)]
    pub swift: SwiftConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub classifiers: Vec<ClassifierRule>,
}
```

- [ ] **Step 6: Add FieldSet to RenderOptions**

In `grapha/src/render.rs`:

```rust
use crate::fields::FieldSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderOptions {
    color_enabled: bool,
    pub fields: FieldSet,
}
```

Update `plain()` and `color()` constructors to include `fields: FieldSet::default()`.

Add a builder method:

```rust
pub const fn with_fields(mut self, fields: FieldSet) -> Self {
    self.fields = fields;
    self
}
```

- [ ] **Step 7: Add --fields flag to CLI commands**

Add `#[arg(long)] fields: Option<String>` to `Context`, `Impact`, `Search`, and all Flow subcommands. In the handlers, resolve FieldSet:

```rust
let field_set = match fields {
    Some(f) => FieldSet::parse(&f),
    None => {
        let cfg = config::load_config(&path);
        if cfg.output.default_fields.is_empty() {
            FieldSet::default()
        } else {
            FieldSet::from_config(&cfg.output.default_fields)
        }
    }
};
let render_opts = render_options.with_fields(field_set);
```

- [ ] **Step 8: Update render functions to use FieldSet**

This is a large but mechanical change. In each `render_*_with_options` function, conditionally include fields based on `options.fields`. For example, in the SymbolRef rendering, only show file if `options.fields.file`, module if `options.fields.module`, etc.

For JSON output, conditionally add fields to the serde output. Update `SymbolRef` and `SymbolInfo` to carry optional fields and use `#[serde(skip_serializing_if = "Option::is_none")]`.

- [ ] **Step 9: Run full tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 10: Commit**

```bash
git add grapha/src/fields.rs grapha/src/render.rs grapha/src/config.rs grapha/src/main.rs
git commit -m "feat(output): add --fields flag for customizable output columns"
```

---

## Phase 5: Cross-Module Graph Analysis

### Task 9: External Repo Configuration

**Files:**
- Modify: `grapha/src/config.rs` (add external config)
- Test: `grapha/tests/config_test.rs` (extend)

- [ ] **Step 1: Write failing test for external config**

```rust
// Add to grapha/tests/config_test.rs
#[test]
fn parse_external_repos() {
    let toml_str = r#"
[[external]]
name = "FrameUI"
path = "/path/to/frameui"

[[external]]
name = "FrameNetwork"
path = "/path/to/framenetwork"
"#;
    let config: grapha::config::GraphaConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.external.len(), 2);
    assert_eq!(config.external[0].name, "FrameUI");
    assert_eq!(config.external[1].path, "/path/to/framenetwork");
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p grapha --test config_test -- parse_external_repos`
Expected: FAIL

- [ ] **Step 3: Add ExternalRepo to config**

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ExternalRepo {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GraphaConfig {
    #[serde(default)]
    pub swift: SwiftConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub external: Vec<ExternalRepo>,
    #[serde(default)]
    pub classifiers: Vec<ClassifierRule>,
}
```

- [ ] **Step 4: Run test**

Run: `cargo test -p grapha --test config_test -- parse_external_repos`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add grapha/src/config.rs grapha/tests/config_test.rs
git commit -m "feat(config): add [[external]] repo configuration"
```

---

### Task 10: External Repo Discovery and Extraction

**Files:**
- Modify: `grapha/src/main.rs:253-391` (run_pipeline — discovery + extraction)

- [ ] **Step 1: Add external file discovery to run_pipeline**

After the main project file discovery (line 258), add:

```rust
let cfg = config::load_config(path);

// Discover external repo files
let mut external_files: Vec<PathBuf> = Vec::new();
let mut external_count = 0usize;
let mut external_repos = 0usize;
for ext in &cfg.external {
    let ext_path = Path::new(&ext.path);
    if !ext_path.exists() {
        if verbose {
            eprintln!(
                "  \x1b[33m!\x1b[0m external repo '{}' not found at {}, skipping",
                ext.name, ext.path
            );
        }
        continue;
    }
    match grapha_core::pipeline::discover_files(ext_path, &registry) {
        Ok(ext_files) => {
            external_count += ext_files.len();
            external_repos += 1;
            external_files.extend(ext_files);
        }
        Err(e) => {
            if verbose {
                eprintln!(
                    "  \x1b[33m!\x1b[0m failed to discover files in '{}': {e}",
                    ext.name
                );
            }
        }
    }
}

let all_files: Vec<PathBuf> = files.into_iter().chain(external_files).collect();
```

Update the progress message:

```rust
if verbose {
    let msg = if external_count > 0 {
        format!(
            "discovered {} files + {} external ({} repos)",
            all_files.len() - external_count,
            external_count,
            external_repos,
        )
    } else {
        format!("discovered {} files", all_files.len())
    };
    progress::done(&msg, t);
}
```

- [ ] **Step 2: Merge external module maps**

After the main module map discovery:

```rust
let mut module_map = grapha_core::discover_modules(&registry, &project_context)?;
for ext in &cfg.external {
    let ext_path = Path::new(&ext.path);
    if !ext_path.exists() {
        continue;
    }
    let ext_context = grapha_core::project_context(ext_path);
    match grapha_core::discover_modules(&registry, &ext_context) {
        Ok(ext_modules) => module_map.merge(ext_modules),
        Err(_) => {}
    }
}
```

- [ ] **Step 3: Use all_files in the par_iter extraction**

Replace `files.par_iter()` with `all_files.par_iter()`. The module map now covers both main and external repos, so `file_context` will resolve correctly for external files.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add grapha/src/main.rs
git commit -m "feat(cross-module): discover and extract external repos from config"
```

---

### Task 11: File Map Command

**Files:**
- Create: `grapha/src/query/map.rs`
- Modify: `grapha/src/main.rs` (add Map subcommand)
- Test: `grapha/tests/map_test.rs` (new)

- [ ] **Step 1: Write failing test**

```rust
// grapha/tests/map_test.rs
use grapha_core::graph::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn file_map_groups_by_directory() {
    let graph = Graph {
        version: "test".to_string(),
        nodes: vec![
            make_node("a", "src/gift/service.swift", Some("LamaLudo")),
            make_node("b", "src/gift/model.swift", Some("LamaLudo")),
            make_node("c", "src/chat/view.swift", Some("LamaLudo")),
        ],
        edges: vec![],
    };
    let map = grapha::query::map::file_map(&graph, None);
    assert_eq!(map.len(), 1); // one module
    let lama = &map["LamaLudo"];
    assert!(lama.iter().any(|g| g.directory == "src/gift/" && g.file_count == 2));
    assert!(lama.iter().any(|g| g.directory == "src/chat/" && g.file_count == 1));
}

fn make_node(id: &str, file: &str, module: Option<&str>) -> Node {
    Node {
        id: id.to_string(),
        kind: NodeKind::Function,
        name: id.to_string(),
        file: PathBuf::from(file),
        span: Span { start: [0, 0], end: [10, 0] },
        visibility: Visibility::Public,
        metadata: HashMap::new(),
        role: None,
        signature: None,
        doc_comment: None,
        module: module.map(String::from),
        snippet: None,
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p grapha --test map_test`
Expected: FAIL

- [ ] **Step 3: Implement file_map**

```rust
// grapha/src/query/map.rs
use grapha_core::graph::Graph;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct DirectoryGroup {
    pub directory: String,
    pub file_count: usize,
    pub symbol_count: usize,
}

/// Returns module_name -> list of directory groups
pub fn file_map(
    graph: &Graph,
    module_filter: Option<&str>,
) -> BTreeMap<String, Vec<DirectoryGroup>> {
    let mut module_dirs: BTreeMap<String, BTreeMap<String, (std::collections::HashSet<String>, usize)>> =
        BTreeMap::new();

    for node in &graph.nodes {
        let module = node.module.as_deref().unwrap_or("(unknown)");
        if let Some(filter) = module_filter {
            if module != filter {
                continue;
            }
        }
        let file_str = node.file.to_string_lossy().to_string();
        let dir = match file_str.rfind('/') {
            Some(pos) => &file_str[..=pos],
            None => "",
        };
        let entry = module_dirs
            .entry(module.to_string())
            .or_default()
            .entry(dir.to_string())
            .or_insert_with(|| (std::collections::HashSet::new(), 0));
        entry.0.insert(file_str);
        entry.1 += 1;
    }

    module_dirs
        .into_iter()
        .map(|(module, dirs)| {
            let mut groups: Vec<DirectoryGroup> = dirs
                .into_iter()
                .map(|(dir, (files, symbol_count))| DirectoryGroup {
                    directory: dir,
                    file_count: files.len(),
                    symbol_count,
                })
                .collect();
            groups.sort_by(|a, b| b.symbol_count.cmp(&a.symbol_count));
            (module, groups)
        })
        .collect()
}
```

Add `pub mod map;` to `grapha/src/query.rs`.

- [ ] **Step 4: Add Map CLI command**

In `grapha/src/main.rs`, add to the `Commands` enum:

```rust
/// Show file/symbol map for orientation in large projects
Map {
    /// Filter by module name
    #[arg(long)]
    module: Option<String>,
    /// Project directory
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
},
```

Add handler:

```rust
Commands::Map { module, path } => {
    let graph = load_graph(&path)?;
    let map = query::map::file_map(&graph, module.as_deref());
    print_json(&map)
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add grapha/src/query/map.rs grapha/src/query.rs grapha/src/main.rs grapha/tests/map_test.rs
git commit -m "feat(query): add file map command for project orientation"
```

---

## Phase 6: MCP Server

### Task 12: MCP Server Infrastructure

**Files:**
- Create: `grapha/src/mcp.rs`
- Create: `grapha/src/mcp/types.rs`
- Create: `grapha/src/mcp/handler.rs`
- Modify: `grapha/src/main.rs` (add --mcp flag to Serve)
- Modify: `grapha/Cargo.toml` (add serde_json features if needed)
- Test: `grapha/tests/mcp_test.rs` (new)

- [ ] **Step 1: Write failing test for MCP tool dispatch**

```rust
// grapha/tests/mcp_test.rs
use serde_json::json;

#[test]
fn mcp_search_symbols_returns_results() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "search_symbols",
            "arguments": {
                "query": "Config",
                "limit": 5
            }
        }
    });
    // Will test the dispatch logic once implemented
    let parsed: grapha::mcp::types::JsonRpcRequest = serde_json::from_value(request).unwrap();
    assert_eq!(parsed.method, "tools/call");
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p grapha --test mcp_test`
Expected: FAIL

- [ ] **Step 3: Create MCP types**

```rust
// grapha/src/mcp/types.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}
```

- [ ] **Step 4: Create MCP handler**

```rust
// grapha/src/mcp/handler.rs
use std::path::Path;
use std::sync::Arc;

use grapha_core::graph::Graph;
use serde_json::json;

use crate::search;
use crate::query;
use super::types::*;

pub struct McpState {
    pub graph: Graph,
    pub search_index: tantivy::Index,
    pub store_path: std::path::PathBuf,
}

pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search_symbols".to_string(),
            description: "Search symbols by name with optional filters".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "kind": { "type": "string", "description": "Filter by kind" },
                    "module": { "type": "string", "description": "Filter by module" },
                    "role": { "type": "string", "description": "Filter by role" },
                    "fuzzy": { "type": "boolean", "description": "Enable fuzzy matching" },
                    "context": { "type": "boolean", "description": "Include snippets and deps" },
                    "limit": { "type": "integer", "description": "Max results", "default": 20 }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "get_symbol_context".to_string(),
            description: "Get 360-degree view of a symbol's relationships".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Symbol name or ID" }
                },
                "required": ["symbol"]
            }),
        },
        ToolDefinition {
            name: "get_impact".to_string(),
            description: "Analyze blast radius of changing a symbol".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Symbol name or ID" },
                    "depth": { "type": "integer", "description": "Max depth", "default": 3 }
                },
                "required": ["symbol"]
            }),
        },
        ToolDefinition {
            name: "get_file_map".to_string(),
            description: "Get directory-level overview of the project".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Filter by module" }
                }
            }),
        },
        ToolDefinition {
            name: "trace".to_string(),
            description: "Trace dataflow from a symbol".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Symbol name or ID" },
                    "depth": { "type": "integer", "description": "Max depth", "default": 10 },
                    "direction": { "type": "string", "enum": ["forward", "reverse"], "default": "forward" }
                },
                "required": ["symbol"]
            }),
        },
        ToolDefinition {
            name: "index_project".to_string(),
            description: "Re-index the project".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "full_rebuild": { "type": "boolean", "default": false }
                }
            }),
        },
    ]
}

pub fn handle_tool_call(
    state: &McpState,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> serde_json::Value {
    match tool_name {
        "search_symbols" => {
            let query = arguments["query"].as_str().unwrap_or("");
            let limit = arguments["limit"].as_u64().unwrap_or(20) as usize;
            let context = arguments["context"].as_bool().unwrap_or(false);
            let options = search::SearchOptions {
                kind: arguments["kind"].as_str().map(String::from),
                module: arguments["module"].as_str().map(String::from),
                file_glob: None,
                role: arguments["role"].as_str().map(String::from),
                fuzzy: arguments["fuzzy"].as_bool().unwrap_or(false),
            };
            let t = std::time::Instant::now();
            match search::search_filtered(&state.search_index, query, limit, &options) {
                Ok(results) => {
                    let elapsed_ms = t.elapsed().as_secs_f64() * 1000.0;
                    if context {
                        let enriched = search::enrich_with_context(&results, &state.graph);
                        json!({
                            "results": enriched,
                            "elapsed_ms": elapsed_ms,
                            "total": enriched.len(),
                        })
                    } else {
                        json!({
                            "results": results,
                            "elapsed_ms": elapsed_ms,
                            "total": results.len(),
                        })
                    }
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        "get_symbol_context" => {
            let symbol = arguments["symbol"].as_str().unwrap_or("");
            match query::context::query_context(&state.graph, symbol) {
                Ok(result) => serde_json::to_value(&result).unwrap_or(json!({"error": "serialize failed"})),
                Err(e) => json!({ "error": format!("{e:?}") }),
            }
        }
        "get_impact" => {
            let symbol = arguments["symbol"].as_str().unwrap_or("");
            let depth = arguments["depth"].as_u64().unwrap_or(3) as usize;
            match query::impact::query_impact(&state.graph, symbol, depth) {
                Ok(result) => serde_json::to_value(&result).unwrap_or(json!({"error": "serialize failed"})),
                Err(e) => json!({ "error": format!("{e:?}") }),
            }
        }
        "get_file_map" => {
            let module = arguments["module"].as_str();
            let map = query::map::file_map(&state.graph, module);
            serde_json::to_value(&map).unwrap_or(json!({"error": "serialize failed"}))
        }
        "trace" => {
            let symbol = arguments["symbol"].as_str().unwrap_or("");
            let depth = arguments["depth"].as_u64().unwrap_or(10) as usize;
            let direction = arguments["direction"].as_str().unwrap_or("forward");
            match direction {
                "forward" => {
                    match query::trace::query_trace(&state.graph, symbol, depth) {
                        Ok(result) => serde_json::to_value(&result).unwrap_or(json!({"error": "serialize failed"})),
                        Err(e) => json!({ "error": format!("{e:?}") }),
                    }
                }
                "reverse" => {
                    match query::reverse::query_reverse(&state.graph, symbol, Some(depth)) {
                        Ok(result) => serde_json::to_value(&result).unwrap_or(json!({"error": "serialize failed"})),
                        Err(e) => json!({ "error": format!("{e:?}") }),
                    }
                }
                other => json!({ "error": format!("unknown direction: {other}") }),
            }
        }
        _ => json!({ "error": format!("unknown tool: {tool_name}") }),
    }
}
```

- [ ] **Step 5: Create MCP server main loop**

```rust
// grapha/src/mcp.rs
pub mod handler;
pub mod types;

use std::io::{self, BufRead, Write};
use handler::McpState;
use types::*;

pub fn run_mcp_server(state: McpState) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("parse error: {e}"),
                );
                writeln!(stdout, "{}", serde_json::to_string(&err)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let response = match request.method.as_str() {
            "initialize" => JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "grapha",
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
            ),
            "tools/list" => {
                let tools = handler::tool_definitions();
                JsonRpcResponse::success(request.id, serde_json::json!({ "tools": tools }))
            }
            "tools/call" => {
                let tool_name = request.params["name"].as_str().unwrap_or("");
                let arguments = &request.params["arguments"];
                let result = handler::handle_tool_call(&state, tool_name, arguments);
                JsonRpcResponse::success(
                    request.id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&result)?
                        }]
                    }),
                )
            }
            "notifications/initialized" | "notifications/cancelled" => continue,
            _ => JsonRpcResponse::error(
                request.id,
                -32601,
                format!("method not found: {}", request.method),
            ),
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}
```

- [ ] **Step 6: Add --mcp flag to Serve command**

In `grapha/src/main.rs`:

```rust
Serve {
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
    #[arg(long, default_value = "8080")]
    port: u16,
    /// Run as MCP server over stdio
    #[arg(long)]
    mcp: bool,
},
```

Update the serve handler:

```rust
Commands::Serve { path, port, mcp } => {
    let graph = load_graph(&path)?;
    let search_index_path = path.join(".grapha").join("search_index");
    let search_index = tantivy::Index::open_in_dir(&search_index_path)
        .context("search index not found — run `grapha index` first")?;

    if mcp {
        let state = mcp::McpState {
            graph,
            search_index,
            store_path: path.join(".grapha"),
        };
        mcp::run_mcp_server(state)
    } else {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(serve::run(graph, search_index, port))
    }
}
```

Add `mod mcp;` to the module declarations.

- [ ] **Step 7: Run MCP type test**

Run: `cargo test -p grapha --test mcp_test`
Expected: PASS

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: All pass

- [ ] **Step 9: Commit**

```bash
git add grapha/src/mcp.rs grapha/src/mcp/ grapha/src/main.rs grapha/tests/mcp_test.rs
git commit -m "feat(mcp): add MCP server mode with 6 tools over stdio"
```

---

## Phase 7: Nodus Package

### Task 13: Create Nodus Package Files

**Files:**
- Create: `nodus/skills/grapha.md`
- Create: `nodus/rules/grapha-workflow.md`
- Create: `nodus/commands/index.md`
- Create: `nodus/commands/search.md`
- Create: `nodus/commands/impact.md`

- [ ] **Step 1: Create skills/grapha.md**

```markdown
---
name: grapha
description: Use grapha for symbol search, context lookup, and impact analysis before reading full files
---

## When to use

Before exploring an unfamiliar part of the codebase or modifying symbols.

## Workflow

1. **Search first:** Run `grapha search "<query>" --context` to find relevant symbols with snippets
2. **Understand relationships:** Run `grapha context <symbol>` to see callers, callees, and dependencies
3. **Check impact before changes:** Run `grapha impact <symbol>` to understand blast radius
4. **Orient in large projects:** Run `grapha map` to see module/directory overview
5. **Read only what you need:** Open specific files and line ranges from search results

## Tips

- Use `--kind function` to narrow search to functions only
- Use `--module ModuleName` to search within a specific module
- Use `--fuzzy` if you're unsure of exact spelling
- After significant code changes, run `grapha index .` to keep the graph fresh
```

- [ ] **Step 2: Create rules/grapha-workflow.md**

```markdown
# Grapha Workflow

- When exploring an unfamiliar part of the codebase, prefer `grapha search` and `grapha context` over reading entire files
- Before modifying any public API, run `grapha impact` to estimate change scope
- After significant code changes, run `grapha index .` to keep the graph fresh
- Use `grapha map` to orient in unfamiliar modules before diving into files
- When searching for a symbol, start with `grapha search` — it's faster and more precise than grep for symbol-level queries
```

- [ ] **Step 3: Create commands/index.md**

```markdown
---
name: index
description: Re-index the project with grapha
---
Run `grapha index .` and report the summary (nodes, edges, time).
```

- [ ] **Step 4: Create commands/search.md**

```markdown
---
name: search
description: Search project symbols with grapha
---
Run `grapha search "$ARGS" --context` and present the results.
If no results found, retry with `--fuzzy` flag.
```

- [ ] **Step 5: Create commands/impact.md**

```markdown
---
name: impact
description: Analyze impact of changing a symbol
---
Run `grapha impact "$ARGS" --depth 3` and summarize:
- Direct dependents (depth 1)
- Indirect dependents (depth 2+)
- Whether any entry points are affected
```

- [ ] **Step 6: Commit**

```bash
git add nodus/
git commit -m "feat(nodus): add nodus package with skills, rules, and commands"
```

---

## Final Verification

### Task 14: Integration Test and Cleanup

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings

- [ ] **Step 3: Run fmt check**

Run: `cargo fmt -- --check`
Expected: No formatting issues

- [ ] **Step 4: Build release**

Run: `cargo build --release`
Expected: Clean build

- [ ] **Step 5: Manual smoke test with a real project**

Run against the grapha project itself:

```bash
cargo run -p grapha -- index .
cargo run -p grapha -- search "Node" --kind struct --context
cargo run -p grapha -- search "insert" --kind function --fuzzy
cargo run -p grapha -- map
cargo run -p grapha -- symbol context Node
cargo run -p grapha -- symbol impact Node --depth 2
```

Verify: search results include module/role fields, context mode shows snippets, timing is printed, map shows directory groups.

- [ ] **Step 6: Final commit if any fixes needed**

```bash
git add -A
git commit -m "chore: integration test fixes and cleanup"
```
