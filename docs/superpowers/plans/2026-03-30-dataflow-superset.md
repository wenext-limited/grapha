# Dataflow Analysis Superset — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make grapha a superset of `wenext-api-dataflow-explore` capabilities for Rust and Swift, with dataflow tracing, condition tracking, entry point detection, cross-module support, and an embedded web UI.

**Architecture:** Enrich the existing graph model with optional dataflow fields (backward compatible). Add a classify post-pass after merge. Build trace/reverse queries on top of the enriched graph. Embed an axum web server with vis-network frontend compiled into the binary.

**Tech Stack:** Rust, tree-sitter, axum, tokio, vis-network (JS), toml crate, existing deps (rusqlite, tantivy, clap, serde)

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `src/classify.rs` | `Classifier` trait, `CompositeClassifier`, `Classification` types |
| `src/classify/swift.rs` | Built-in Swift framework patterns (URLSession, CoreData, etc.) |
| `src/classify/rust.rs` | Built-in Rust framework patterns (std::fs, rusqlite, etc.) |
| `src/classify/toml.rs` | User-defined classifier rules from `grapha.toml` |
| `src/config.rs` | `grapha.toml` parsing (classifiers, entry points) |
| `src/query/trace.rs` | Forward dataflow tracing (entry → terminals) |
| `src/query/reverse.rs` | Reverse query (symbol → affected entry points) |
| `src/query/entries.rs` | List auto-detected entry points |
| `src/module.rs` | Module map building (Package.swift / Cargo.toml discovery) |
| `src/serve.rs` | `grapha serve` HTTP server (axum) |
| `src/serve/api.rs` | REST API handlers |
| `src/serve/assets.rs` | Embedded static assets (include_str!) |
| `src/serve/web/index.html` | Single-page app with vis-network |
| `tests/fixtures/dataflow_swift.swift` | Swift fixture with dataflow patterns |
| `tests/fixtures/dataflow_rust.rs` | Rust fixture with dataflow patterns |
| `tests/fixtures/multi_module/` | Multi-package Swift fixture |

### Modified Files

| File | Changes |
|------|---------|
| `src/graph.rs` | Add `NodeRole`, `TerminalKind`, `FlowDirection` enums; extend `Node` and `Edge` with optional fields |
| `src/extract.rs` | Add `condition` and `async_boundary` to `ExtractionResult` context |
| `src/extract/swift.rs` | Extract conditions, async boundaries, signatures, doc comments, entry point detection |
| `src/extract/rust.rs` | Extract conditions, async boundaries, signatures, doc comments, entry point detection |
| `src/merge.rs` | Module-aware resolution with confidence tiers |
| `src/store/sqlite.rs` | Add nullable columns for new fields, schema migration |
| `src/store/json.rs` | No changes needed (serde handles Option fields automatically) |
| `src/compress/group.rs` | Include new fields in grouped output |
| `src/query.rs` | Re-export new query modules |
| `src/main.rs` | Add `trace`, `reverse`, `entries`, `serve` subcommands; load config |
| `src/discover.rs` | Detect `Package.swift` / `Cargo.toml` during walk |
| `Cargo.toml` | Add `axum`, `tokio`, `toml` dependencies |

---

## Phase 1: Enriched Graph Model

### Task 1: Extend graph types with dataflow fields

**Files:**
- Modify: `src/graph.rs`

- [ ] **Step 1: Write failing test for new enum serialization**

```rust
// Add to the existing #[cfg(test)] mod tests in src/graph.rs

#[test]
fn node_role_serializes_as_snake_case() {
    let json = serde_json::to_string(&NodeRole::EntryPoint).unwrap();
    assert_eq!(json, "\"entry_point\"");

    let json = serde_json::to_string(&NodeRole::Terminal(TerminalKind::Network)).unwrap();
    assert!(json.contains("terminal"));

    let json = serde_json::to_string(&NodeRole::Internal).unwrap();
    assert_eq!(json, "\"internal\"");
}

#[test]
fn terminal_kind_serializes_as_snake_case() {
    let json = serde_json::to_string(&TerminalKind::Persistence).unwrap();
    assert_eq!(json, "\"persistence\"");
}

#[test]
fn flow_direction_serializes_as_snake_case() {
    let json = serde_json::to_string(&FlowDirection::ReadWrite).unwrap();
    assert_eq!(json, "\"read_write\"");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib graph::tests::node_role_serializes 2>&1 | head -20`
Expected: FAIL — `NodeRole` not found

- [ ] **Step 3: Add new enums to `src/graph.rs`**

Add these after the existing `Visibility` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    Network,
    Persistence,
    Cache,
    Event,
    Keychain,
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum NodeRole {
    EntryPoint,
    Terminal { kind: TerminalKind },
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    Read,
    Write,
    ReadWrite,
    Pure,
}
```

- [ ] **Step 4: Run the new enum tests to verify they pass**

Run: `cargo test --lib graph::tests::node_role_serializes graph::tests::terminal_kind_serializes graph::tests::flow_direction_serializes`
Expected: PASS (adjust serialization format if `tag = "type"` produces different output — may need `#[serde(untagged)]` or adjust test expectations)

- [ ] **Step 5: Write failing test for extended Node**

```rust
#[test]
fn node_with_optional_fields_round_trips() {
    let node = Node {
        id: "test::main".to_string(),
        kind: NodeKind::Function,
        name: "main".to_string(),
        file: PathBuf::from("test.rs"),
        span: Span { start: [0, 0], end: [5, 1] },
        visibility: Visibility::Public,
        metadata: HashMap::new(),
        role: Some(NodeRole::EntryPoint),
        signature: Some("fn main() -> Result<()>".to_string()),
        doc_comment: Some("Entry point".to_string()),
        module: Some("core".to_string()),
    };
    let json = serde_json::to_string(&node).unwrap();
    let deserialized: Node = serde_json::from_str(&json).unwrap();
    assert_eq!(node, deserialized);
    assert_eq!(deserialized.role, Some(NodeRole::EntryPoint));
}

#[test]
fn node_without_optional_fields_round_trips() {
    let node = Node {
        id: "test::foo".to_string(),
        kind: NodeKind::Function,
        name: "foo".to_string(),
        file: PathBuf::from("test.rs"),
        span: Span { start: [0, 0], end: [5, 1] },
        visibility: Visibility::Public,
        metadata: HashMap::new(),
        role: None,
        signature: None,
        doc_comment: None,
        module: None,
    };
    let json = serde_json::to_string(&node).unwrap();
    assert!(!json.contains("role"));
    let deserialized: Node = serde_json::from_str(&json).unwrap();
    assert_eq!(node, deserialized);
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test --lib graph::tests::node_with_optional 2>&1 | head -20`
Expected: FAIL — `Node` struct doesn't have `role` field

- [ ] **Step 7: Extend `Node` struct**

In `src/graph.rs`, add the new optional fields to `Node`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub file: PathBuf,
    pub span: Span,
    pub visibility: Visibility,
    pub metadata: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<NodeRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
}
```

- [ ] **Step 8: Fix all existing `Node` construction sites**

Every existing `Node { ... }` in the codebase needs the four new fields. Add `role: None, signature: None, doc_comment: None, module: None` to each. Files to update:
- `src/extract/rust.rs` — all `Node { ... }` constructions
- `src/extract/swift.rs` — all `Node { ... }` constructions
- `src/merge.rs` — test helper `make_node`
- `src/compress/prune.rs` — test helper `make_node`
- `src/compress/group.rs` — test helper `make_node`
- `src/query/context.rs` — test `make_graph`
- `src/query/impact.rs` — test `make_chain_graph`
- `src/store/sqlite.rs` — test graph construction
- `src/changes.rs` — if it constructs Nodes
- `tests/` — any integration tests

Search for all construction sites with: `grep -rn "Node {" src/ tests/`

- [ ] **Step 9: Run all tests to verify Node changes compile and pass**

Run: `cargo test 2>&1 | tail -5`
Expected: All 79 existing tests PASS

- [ ] **Step 10: Write failing test for extended Edge**

```rust
#[test]
fn edge_with_optional_fields_round_trips() {
    let edge = Edge {
        source: "a".to_string(),
        target: "b".to_string(),
        kind: EdgeKind::Calls,
        confidence: 0.9,
        direction: Some(FlowDirection::Write),
        operation: Some("insert".to_string()),
        condition: Some("user.is_admin".to_string()),
        async_boundary: Some(true),
    };
    let json = serde_json::to_string(&edge).unwrap();
    let deserialized: Edge = serde_json::from_str(&json).unwrap();
    assert_eq!(edge, deserialized);
}

#[test]
fn edge_without_optional_fields_round_trips() {
    let edge = Edge {
        source: "a".to_string(),
        target: "b".to_string(),
        kind: EdgeKind::Calls,
        confidence: 0.9,
        direction: None,
        operation: None,
        condition: None,
        async_boundary: None,
    };
    let json = serde_json::to_string(&edge).unwrap();
    assert!(!json.contains("direction"));
    let deserialized: Edge = serde_json::from_str(&json).unwrap();
    assert_eq!(edge, deserialized);
}
```

- [ ] **Step 11: Extend `Edge` struct and add new `EdgeKind` variants**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Calls,
    Uses,
    Implements,
    Contains,
    TypeRef,
    Inherits,
    Reads,
    Writes,
    Publishes,
    Subscribes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<FlowDirection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub async_boundary: Option<bool>,
}
```

- [ ] **Step 12: Fix all existing `Edge` construction sites**

Add `direction: None, operation: None, condition: None, async_boundary: None` to every `Edge { ... }`. Same files as Step 8 plus:
- `src/compress/prune.rs` — test edges
- `src/compress/group.rs` — test edges

Search: `grep -rn "Edge {" src/ tests/`

- [ ] **Step 13: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests PASS

- [ ] **Step 14: Commit**

```bash
git add src/graph.rs src/extract/ src/merge.rs src/compress/ src/query/ src/store/ src/changes.rs tests/
git commit -m "feat: enrich graph model with dataflow fields (NodeRole, FlowDirection, conditions)"
```

---

### Task 2: Update SQLite schema for new fields

**Files:**
- Modify: `src/store/sqlite.rs`

- [ ] **Step 1: Write failing test for new fields round-trip**

```rust
#[test]
fn sqlite_store_round_trips_dataflow_fields() {
    use crate::graph::{NodeRole, TerminalKind, FlowDirection};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grapha.db");
    let store = SqliteStore::new(path);

    let graph = Graph {
        version: "0.1.0".to_string(),
        nodes: vec![Node {
            id: "test.rs::fetch".to_string(),
            kind: NodeKind::Function,
            name: "fetch".to_string(),
            file: "test.rs".into(),
            span: Span { start: [0, 0], end: [5, 1] },
            visibility: Visibility::Public,
            metadata: HashMap::new(),
            role: Some(NodeRole::Terminal { kind: TerminalKind::Network }),
            signature: Some("fn fetch(url: &str) -> Result<Response>".to_string()),
            doc_comment: Some("Fetch a URL".to_string()),
            module: Some("network".to_string()),
        }],
        edges: vec![Edge {
            source: "test.rs::main".to_string(),
            target: "test.rs::fetch".to_string(),
            kind: EdgeKind::Calls,
            confidence: 0.9,
            direction: Some(FlowDirection::Read),
            operation: Some("fetch".to_string()),
            condition: Some("url.is_valid()".to_string()),
            async_boundary: Some(true),
        }],
    };

    store.save(&graph).unwrap();
    let loaded = store.load().unwrap();

    assert_eq!(loaded.nodes[0].role, Some(NodeRole::Terminal { kind: TerminalKind::Network }));
    assert_eq!(loaded.nodes[0].signature.as_deref(), Some("fn fetch(url: &str) -> Result<Response>"));
    assert_eq!(loaded.nodes[0].doc_comment.as_deref(), Some("Fetch a URL"));
    assert_eq!(loaded.nodes[0].module.as_deref(), Some("network"));
    assert_eq!(loaded.edges[0].direction, Some(FlowDirection::Read));
    assert_eq!(loaded.edges[0].operation.as_deref(), Some("fetch"));
    assert_eq!(loaded.edges[0].condition.as_deref(), Some("url.is_valid()"));
    assert_eq!(loaded.edges[0].async_boundary, Some(true));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib store::sqlite::tests::sqlite_store_round_trips_dataflow 2>&1 | head -20`
Expected: FAIL — columns don't exist

- [ ] **Step 3: Update `create_tables` with new nullable columns**

In `src/store/sqlite.rs`, update the `CREATE TABLE` statements:

```rust
fn create_tables(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nodes (
            id             TEXT PRIMARY KEY,
            kind           TEXT NOT NULL,
            name           TEXT NOT NULL,
            file           TEXT NOT NULL,
            span_start_line   INTEGER NOT NULL,
            span_start_col    INTEGER NOT NULL,
            span_end_line     INTEGER NOT NULL,
            span_end_col      INTEGER NOT NULL,
            visibility     TEXT NOT NULL,
            metadata       TEXT NOT NULL,
            role           TEXT,
            signature      TEXT,
            doc_comment    TEXT,
            module         TEXT
        );
        CREATE TABLE IF NOT EXISTS edges (
            source         TEXT NOT NULL,
            target         TEXT NOT NULL,
            kind           TEXT NOT NULL,
            confidence     REAL NOT NULL,
            direction      TEXT,
            operation      TEXT,
            condition      TEXT,
            async_boundary INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
        CREATE INDEX IF NOT EXISTS idx_edges_kind   ON edges(kind);
        CREATE INDEX IF NOT EXISTS idx_nodes_name   ON nodes(name);
        CREATE INDEX IF NOT EXISTS idx_nodes_file   ON nodes(file);
        CREATE INDEX IF NOT EXISTS idx_nodes_kind   ON nodes(kind);
        CREATE INDEX IF NOT EXISTS idx_nodes_role   ON nodes(role);
        CREATE INDEX IF NOT EXISTS idx_nodes_module ON nodes(module);",
    )?;
    Ok(())
}
```

- [ ] **Step 4: Update `save` to write new fields**

Update the node INSERT to include new columns:

```rust
let mut stmt = tx.prepare(
    "INSERT OR REPLACE INTO nodes (id, kind, name, file,
        span_start_line, span_start_col, span_end_line, span_end_col,
        visibility, metadata, role, signature, doc_comment, module)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
)?;
for node in &graph.nodes {
    let role_str: Option<String> = node.role.as_ref().map(|r| serde_json::to_string(r)).transpose()?;
    stmt.execute(rusqlite::params![
        node.id,
        enum_to_str(&node.kind)?,
        node.name,
        node.file.to_string_lossy().as_ref(),
        node.span.start[0] as i64,
        node.span.start[1] as i64,
        node.span.end[0] as i64,
        node.span.end[1] as i64,
        enum_to_str(&node.visibility)?,
        serde_json::to_string(&node.metadata)?,
        role_str,
        node.signature,
        node.doc_comment,
        node.module,
    ])?;
}
```

Update the edge INSERT:

```rust
let mut stmt = tx.prepare(
    "INSERT INTO edges (source, target, kind, confidence, direction, operation, condition, async_boundary)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
)?;
for edge in &graph.edges {
    let dir_str: Option<String> = edge.direction.map(|d| enum_to_str(&d)).transpose()?;
    stmt.execute(rusqlite::params![
        edge.source,
        edge.target,
        enum_to_str(&edge.kind)?,
        edge.confidence,
        dir_str,
        edge.operation,
        edge.condition,
        edge.async_boundary.map(|b| b as i64),
    ])?;
}
```

- [ ] **Step 5: Update `load` to read new fields**

Update node loading to read the 4 new columns:

```rust
let mut stmt = conn.prepare(
    "SELECT id, kind, name, file,
            span_start_line, span_start_col, span_end_line, span_end_col,
            visibility, metadata, role, signature, doc_comment, module
     FROM nodes",
)?;
```

And in the row mapping, after building the base node, add:

```rust
let role_str: Option<String> = row.get(10)?;
let role: Option<NodeRole> = role_str
    .map(|s| serde_json::from_str(&s))
    .transpose()
    .map_err(|e| anyhow::anyhow!("invalid node role: {e}"))?;
let signature: Option<String> = row.get(11)?;
let doc_comment: Option<String> = row.get(12)?;
let module: Option<String> = row.get(13)?;
```

Update edge loading similarly:

```rust
let mut stmt = conn.prepare(
    "SELECT source, target, kind, confidence, direction, operation, condition, async_boundary
     FROM edges",
)?;
```

And in the row mapping:

```rust
let dir_str: Option<String> = row.get(4)?;
let direction: Option<FlowDirection> = dir_str
    .map(|s| str_to_enum(&s))
    .transpose()
    .map_err(|e| anyhow::anyhow!("invalid flow direction: {e}"))?;
let operation: Option<String> = row.get(5)?;
let condition: Option<String> = row.get(6)?;
let async_boundary: Option<bool> = row.get::<_, Option<i64>>(7)?.map(|v| v != 0);
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib store::sqlite::tests`
Expected: All SQLite tests PASS including the new one

- [ ] **Step 7: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/store/sqlite.rs
git commit -m "feat: extend SQLite schema for dataflow fields"
```

---

## Phase 2: Configuration System

### Task 3: Parse `grapha.toml` configuration

**Files:**
- Create: `src/config.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `toml` dependency**

In `Cargo.toml`, add to `[dependencies]`:

```toml
toml = "0.8"
```

- [ ] **Step 2: Write failing test for config parsing**

Create `src/config.rs`:

```rust
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GraphaConfig {
    #[serde(default)]
    pub classifiers: Vec<ClassifierRule>,
    #[serde(default)]
    pub entry_points: Vec<EntryPointRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClassifierRule {
    pub pattern: String,
    pub terminal: String,
    pub direction: String,
    pub operation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntryPointRule {
    pub language: String,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub attribute: Option<String>,
}

pub fn load_config(project_root: &Path) -> GraphaConfig {
    let config_path = project_root.join("grapha.toml");
    if !config_path.exists() {
        return GraphaConfig::default();
    }
    match std::fs::read_to_string(&config_path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => GraphaConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_config() {
        let config: GraphaConfig = toml::from_str("").unwrap();
        assert!(config.classifiers.is_empty());
        assert!(config.entry_points.is_empty());
    }

    #[test]
    fn parses_classifier_rules() {
        let toml_str = r#"
[[classifiers]]
pattern = "FirebaseFirestore.*setData"
terminal = "persistence"
direction = "write"
operation = "set"

[[classifiers]]
pattern = "SocketManager.*emit"
terminal = "network"
direction = "write"
operation = "emit"
"#;
        let config: GraphaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.classifiers.len(), 2);
        assert_eq!(config.classifiers[0].pattern, "FirebaseFirestore.*setData");
        assert_eq!(config.classifiers[0].terminal, "persistence");
    }

    #[test]
    fn parses_entry_point_rules() {
        let toml_str = r#"
[[entry_points]]
language = "swift"
pattern = ".*Coordinator.start"

[[entry_points]]
language = "rust"
attribute = "actix_web::get"
"#;
        let config: GraphaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entry_points.len(), 2);
        assert_eq!(config.entry_points[0].language, "swift");
        assert_eq!(config.entry_points[0].pattern.as_deref(), Some(".*Coordinator.start"));
        assert_eq!(config.entry_points[1].attribute.as_deref(), Some("actix_web::get"));
    }

    #[test]
    fn load_config_returns_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path());
        assert!(config.classifiers.is_empty());
    }

    #[test]
    fn load_config_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("grapha.toml"), r#"
[[classifiers]]
pattern = "test"
terminal = "network"
direction = "read"
operation = "get"
"#).unwrap();
        let config = load_config(dir.path());
        assert_eq!(config.classifiers.len(), 1);
    }
}
```

- [ ] **Step 3: Register the module in `src/main.rs`**

Add `mod config;` to the module declarations at the top of `src/main.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib config::tests`
Expected: All 5 config tests PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/config.rs src/main.rs
git commit -m "feat: add grapha.toml configuration parsing"
```

---

## Phase 3: Classifier System

### Task 4: Classifier trait and composite classifier

**Files:**
- Create: `src/classify.rs`

- [ ] **Step 1: Write the classifier trait and types**

Create `src/classify.rs`:

```rust
pub mod rust;
pub mod swift;
pub mod toml_rules;

use std::path::PathBuf;

use crate::graph::{FlowDirection, TerminalKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub terminal_kind: TerminalKind,
    pub direction: FlowDirection,
    pub operation: String,
}

#[derive(Debug, Clone)]
pub struct ClassifyContext {
    pub source_node: String,
    pub file: PathBuf,
    pub arguments: Vec<String>,
}

pub trait Classifier: Send + Sync {
    fn classify(&self, call_target: &str, context: &ClassifyContext) -> Option<Classification>;
}

/// Chains multiple classifiers; first match wins.
pub struct CompositeClassifier {
    classifiers: Vec<Box<dyn Classifier>>,
}

impl CompositeClassifier {
    pub fn new(classifiers: Vec<Box<dyn Classifier>>) -> Self {
        Self { classifiers }
    }

    pub fn classify(&self, call_target: &str, context: &ClassifyContext) -> Option<Classification> {
        self.classifiers
            .iter()
            .find_map(|c| c.classify(call_target, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysNetwork;
    impl Classifier for AlwaysNetwork {
        fn classify(&self, _target: &str, _ctx: &ClassifyContext) -> Option<Classification> {
            Some(Classification {
                terminal_kind: TerminalKind::Network,
                direction: FlowDirection::Read,
                operation: "fetch".to_string(),
            })
        }
    }

    struct NeverMatch;
    impl Classifier for NeverMatch {
        fn classify(&self, _target: &str, _ctx: &ClassifyContext) -> Option<Classification> {
            None
        }
    }

    fn test_context() -> ClassifyContext {
        ClassifyContext {
            source_node: "test::main".to_string(),
            file: PathBuf::from("test.rs"),
            arguments: vec![],
        }
    }

    #[test]
    fn composite_returns_first_match() {
        let comp = CompositeClassifier::new(vec![
            Box::new(NeverMatch),
            Box::new(AlwaysNetwork),
        ]);
        let result = comp.classify("anything", &test_context());
        assert!(result.is_some());
        assert_eq!(result.unwrap().terminal_kind, TerminalKind::Network);
    }

    #[test]
    fn composite_returns_none_when_no_match() {
        let comp = CompositeClassifier::new(vec![Box::new(NeverMatch)]);
        assert!(comp.classify("anything", &test_context()).is_none());
    }

    #[test]
    fn first_classifier_wins_over_later() {
        struct AlwaysPersistence;
        impl Classifier for AlwaysPersistence {
            fn classify(&self, _target: &str, _ctx: &ClassifyContext) -> Option<Classification> {
                Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Write,
                    operation: "save".to_string(),
                })
            }
        }

        let comp = CompositeClassifier::new(vec![
            Box::new(AlwaysPersistence),
            Box::new(AlwaysNetwork),
        ]);
        let result = comp.classify("anything", &test_context()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
    }
}
```

- [ ] **Step 2: Register module in `src/main.rs`**

Add `mod classify;` to the module declarations.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib classify::tests`
Expected: All 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/classify.rs src/main.rs
git commit -m "feat: add Classifier trait and CompositeClassifier"
```

### Task 5: Swift classifier

**Files:**
- Create: `src/classify/swift.rs`

- [ ] **Step 1: Write the Swift classifier with tests**

Create `src/classify/swift.rs`:

```rust
use super::{Classification, Classifier, ClassifyContext};
use crate::graph::{FlowDirection, TerminalKind};

pub struct SwiftClassifier;

impl Classifier for SwiftClassifier {
    fn classify(&self, call_target: &str, _context: &ClassifyContext) -> Option<Classification> {
        // Network
        if matches_any(call_target, &["URLSession", "AF.request", "AF.download", "Moya", "dataTask"]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Network,
                direction: FlowDirection::Read,
                operation: "fetch".to_string(),
            });
        }
        if matches_any(call_target, &["AF.upload", "upload"]) && contains_any(call_target, &["URLSession", "AF"]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Network,
                direction: FlowDirection::Write,
                operation: "upload".to_string(),
            });
        }

        // Persistence — CoreData
        if contains(call_target, "NSManagedObjectContext") || contains(call_target, "viewContext") || contains(call_target, "backgroundContext") {
            if contains_any(call_target, &["save", "insert", "delete"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Write,
                    operation: if contains(call_target, "delete") { "delete" } else { "save" }.to_string(),
                });
            }
            if contains_any(call_target, &["fetch", "count", "object"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Read,
                    operation: "fetch".to_string(),
                });
            }
        }

        // Persistence — Realm
        if contains(call_target, "realm") {
            if contains_any(call_target, &[".write", ".add", ".delete"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Write,
                    operation: "write".to_string(),
                });
            }
            if contains_any(call_target, &[".objects", ".object("]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Read,
                    operation: "fetch".to_string(),
                });
            }
        }

        // Persistence — UserDefaults
        if contains(call_target, "UserDefaults") {
            if contains_any(call_target, &["set(", "setValue", "removeObject"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Write,
                    operation: "set".to_string(),
                });
            }
            if contains_any(call_target, &["string(forKey", "integer(forKey", "bool(forKey", "object(forKey", "value(forKey", "data(forKey"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Read,
                    operation: "get".to_string(),
                });
            }
        }

        // Keychain
        if contains_any(call_target, &["KeychainWrapper", "SecItemAdd", "SecItemUpdate", "Keychain"]) {
            if contains_any(call_target, &["set", "SecItemAdd", "SecItemUpdate", "save"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Keychain,
                    direction: FlowDirection::Write,
                    operation: "set".to_string(),
                });
            }
            if contains_any(call_target, &["get", "SecItemCopyMatching", "string(forKey"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Keychain,
                    direction: FlowDirection::Read,
                    operation: "get".to_string(),
                });
            }
        }

        // Events — NotificationCenter
        if contains(call_target, "NotificationCenter") {
            if contains(call_target, "post") {
                return Some(Classification {
                    terminal_kind: TerminalKind::Event,
                    direction: FlowDirection::Write,
                    operation: "publish".to_string(),
                });
            }
            if contains_any(call_target, &["addObserver", "publisher(for"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Event,
                    direction: FlowDirection::Read,
                    operation: "subscribe".to_string(),
                });
            }
        }

        // Events — Combine
        if contains_any(call_target, &["PassthroughSubject", "CurrentValueSubject"]) {
            if contains(call_target, "send") {
                return Some(Classification {
                    terminal_kind: TerminalKind::Event,
                    direction: FlowDirection::Write,
                    operation: "publish".to_string(),
                });
            }
        }

        // Cache — NSCache
        if contains(call_target, "NSCache") {
            if contains(call_target, "setObject") {
                return Some(Classification {
                    terminal_kind: TerminalKind::Cache,
                    direction: FlowDirection::Write,
                    operation: "set".to_string(),
                });
            }
            if contains(call_target, "object(forKey") {
                return Some(Classification {
                    terminal_kind: TerminalKind::Cache,
                    direction: FlowDirection::Read,
                    operation: "get".to_string(),
                });
            }
        }

        None
    }
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

fn matches_any(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| haystack.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> ClassifyContext {
        ClassifyContext {
            source_node: "test".to_string(),
            file: PathBuf::from("test.swift"),
            arguments: vec![],
        }
    }

    #[test]
    fn classifies_urlsession_as_network_read() {
        let c = SwiftClassifier;
        let result = c.classify("URLSession.shared.dataTask", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Network);
        assert_eq!(result.direction, FlowDirection::Read);
    }

    #[test]
    fn classifies_userdefaults_set_as_persistence_write() {
        let c = SwiftClassifier;
        let result = c.classify("UserDefaults.standard.set(value, forKey: key)", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn classifies_userdefaults_read() {
        let c = SwiftClassifier;
        let result = c.classify("UserDefaults.standard.string(forKey: \"name\")", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Read);
    }

    #[test]
    fn classifies_notification_post_as_event_write() {
        let c = SwiftClassifier;
        let result = c.classify("NotificationCenter.default.post", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Event);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn classifies_nscache_set_as_cache_write() {
        let c = SwiftClassifier;
        let result = c.classify("NSCache.setObject", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Cache);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn returns_none_for_unknown() {
        let c = SwiftClassifier;
        assert!(c.classify("myCustomFunction", &ctx()).is_none());
    }

    #[test]
    fn classifies_keychain_write() {
        let c = SwiftClassifier;
        let result = c.classify("KeychainWrapper.standard.set(token, forKey: \"auth\")", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Keychain);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn classifies_combine_subject_send() {
        let c = SwiftClassifier;
        let result = c.classify("PassthroughSubject.send(value)", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Event);
        assert_eq!(result.direction, FlowDirection::Write);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib classify::swift::tests`
Expected: All 8 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/classify/swift.rs
git commit -m "feat: add built-in Swift classifier patterns"
```

### Task 6: Rust classifier

**Files:**
- Create: `src/classify/rust.rs`

- [ ] **Step 1: Write the Rust classifier with tests**

Create `src/classify/rust.rs`:

```rust
use super::{Classification, Classifier, ClassifyContext};
use crate::graph::{FlowDirection, TerminalKind};

pub struct RustClassifier;

impl Classifier for RustClassifier {
    fn classify(&self, call_target: &str, _context: &ClassifyContext) -> Option<Classification> {
        // Persistence — std::fs
        if contains_any(call_target, &["fs::read", "fs::read_to_string", "File::open"]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Persistence,
                direction: FlowDirection::Read,
                operation: "read".to_string(),
            });
        }
        if contains_any(call_target, &["fs::write", "fs::create_dir", "File::create"]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Persistence,
                direction: FlowDirection::Write,
                operation: "write".to_string(),
            });
        }

        // Persistence — rusqlite
        if contains(call_target, "Connection") || contains(call_target, "conn") {
            if contains_any(call_target, &["execute", "execute_batch"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Write,
                    operation: "execute".to_string(),
                });
            }
            if contains_any(call_target, &["query_row", "query_map", "prepare"]) {
                return Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Read,
                    operation: "query".to_string(),
                });
            }
        }

        // Search — tantivy
        if contains(call_target, "IndexWriter") {
            return Some(Classification {
                terminal_kind: TerminalKind::Search,
                direction: FlowDirection::Write,
                operation: "index".to_string(),
            });
        }
        if contains_any(call_target, &["Searcher", "search("]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Search,
                direction: FlowDirection::Read,
                operation: "search".to_string(),
            });
        }

        // Events — tokio channels
        if contains_any(call_target, &["Sender::send", "sender.send", "tx.send"]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Event,
                direction: FlowDirection::Write,
                operation: "send".to_string(),
            });
        }
        if contains_any(call_target, &["Receiver::recv", "receiver.recv", "rx.recv"]) {
            return Some(Classification {
                terminal_kind: TerminalKind::Event,
                direction: FlowDirection::Read,
                operation: "receive".to_string(),
            });
        }

        // Network — reqwest
        if contains_any(call_target, &["reqwest", "Client::get", "Client::post"]) {
            let direction = if contains_any(call_target, &["post", "put", "patch", "delete"]) {
                FlowDirection::Write
            } else {
                FlowDirection::Read
            };
            return Some(Classification {
                terminal_kind: TerminalKind::Network,
                direction,
                operation: "fetch".to_string(),
            });
        }

        None
    }
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> ClassifyContext {
        ClassifyContext {
            source_node: "test".to_string(),
            file: PathBuf::from("test.rs"),
            arguments: vec![],
        }
    }

    #[test]
    fn classifies_fs_read_as_persistence_read() {
        let c = RustClassifier;
        let result = c.classify("fs::read_to_string", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Read);
    }

    #[test]
    fn classifies_fs_write_as_persistence_write() {
        let c = RustClassifier;
        let result = c.classify("fs::write", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn classifies_rusqlite_execute_as_write() {
        let c = RustClassifier;
        let result = c.classify("Connection::execute", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn classifies_rusqlite_query_as_read() {
        let c = RustClassifier;
        let result = c.classify("Connection::query_row", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Read);
    }

    #[test]
    fn classifies_tantivy_index_writer() {
        let c = RustClassifier;
        let result = c.classify("IndexWriter::add_document", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Search);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn classifies_channel_send() {
        let c = RustClassifier;
        let result = c.classify("tx.send(msg)", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Event);
        assert_eq!(result.direction, FlowDirection::Write);
    }

    #[test]
    fn returns_none_for_unknown() {
        let c = RustClassifier;
        assert!(c.classify("my_custom_function", &ctx()).is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib classify::rust::tests`
Expected: All 7 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/classify/rust.rs
git commit -m "feat: add built-in Rust classifier patterns"
```

### Task 7: TOML-based user classifier

**Files:**
- Create: `src/classify/toml_rules.rs`

- [ ] **Step 1: Write the TOML classifier with tests**

Create `src/classify/toml_rules.rs`:

```rust
use crate::config::ClassifierRule;
use crate::graph::{FlowDirection, TerminalKind};

use super::{Classification, Classifier, ClassifyContext};

pub struct TomlClassifier {
    rules: Vec<CompiledRule>,
}

struct CompiledRule {
    pattern: regex::Regex,
    terminal_kind: TerminalKind,
    direction: FlowDirection,
    operation: String,
}

impl TomlClassifier {
    pub fn new(rules: &[ClassifierRule]) -> Self {
        let compiled = rules
            .iter()
            .filter_map(|r| {
                let pattern = regex::Regex::new(&r.pattern).ok()?;
                let terminal_kind = parse_terminal(&r.terminal)?;
                let direction = parse_direction(&r.direction)?;
                Some(CompiledRule {
                    pattern,
                    terminal_kind,
                    direction,
                    operation: r.operation.clone(),
                })
            })
            .collect();
        Self { rules: compiled }
    }
}

impl Classifier for TomlClassifier {
    fn classify(&self, call_target: &str, _context: &ClassifyContext) -> Option<Classification> {
        self.rules.iter().find_map(|rule| {
            if rule.pattern.is_match(call_target) {
                Some(Classification {
                    terminal_kind: rule.terminal_kind,
                    direction: rule.direction,
                    operation: rule.operation.clone(),
                })
            } else {
                None
            }
        })
    }
}

fn parse_terminal(s: &str) -> Option<TerminalKind> {
    match s {
        "network" => Some(TerminalKind::Network),
        "persistence" => Some(TerminalKind::Persistence),
        "cache" => Some(TerminalKind::Cache),
        "event" => Some(TerminalKind::Event),
        "keychain" => Some(TerminalKind::Keychain),
        "search" => Some(TerminalKind::Search),
        _ => None,
    }
}

fn parse_direction(s: &str) -> Option<FlowDirection> {
    match s {
        "read" => Some(FlowDirection::Read),
        "write" => Some(FlowDirection::Write),
        "read_write" => Some(FlowDirection::ReadWrite),
        "pure" => Some(FlowDirection::Pure),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> ClassifyContext {
        ClassifyContext {
            source_node: "test".to_string(),
            file: PathBuf::from("test.swift"),
            arguments: vec![],
        }
    }

    #[test]
    fn matches_regex_pattern() {
        let rules = vec![ClassifierRule {
            pattern: "Firebase.*setData".to_string(),
            terminal: "persistence".to_string(),
            direction: "write".to_string(),
            operation: "set".to_string(),
        }];
        let classifier = TomlClassifier::new(&rules);
        let result = classifier.classify("FirebaseFirestore.setData(document)", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Persistence);
        assert_eq!(result.direction, FlowDirection::Write);
        assert_eq!(result.operation, "set");
    }

    #[test]
    fn returns_none_when_no_match() {
        let rules = vec![ClassifierRule {
            pattern: "Firebase.*".to_string(),
            terminal: "persistence".to_string(),
            direction: "write".to_string(),
            operation: "set".to_string(),
        }];
        let classifier = TomlClassifier::new(&rules);
        assert!(classifier.classify("URLSession.dataTask", &ctx()).is_none());
    }

    #[test]
    fn skips_invalid_rules() {
        let rules = vec![
            ClassifierRule {
                pattern: "[invalid regex".to_string(),
                terminal: "persistence".to_string(),
                direction: "write".to_string(),
                operation: "set".to_string(),
            },
            ClassifierRule {
                pattern: "valid.*".to_string(),
                terminal: "network".to_string(),
                direction: "read".to_string(),
                operation: "get".to_string(),
            },
        ];
        let classifier = TomlClassifier::new(&rules);
        let result = classifier.classify("valid_call", &ctx()).unwrap();
        assert_eq!(result.terminal_kind, TerminalKind::Network);
    }
}
```

- [ ] **Step 2: Add `regex` dependency to `Cargo.toml`**

```toml
regex = "1"
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib classify::toml_rules::tests`
Expected: All 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/classify/toml_rules.rs Cargo.toml
git commit -m "feat: add TOML-based user-defined classifier"
```

### Task 8: Classify post-pass — enrich graph edges after merge

**Files:**
- Create: `src/classify/pass.rs` (the post-merge classification pass)
- Modify: `src/classify.rs` (add `pub mod pass;`)

- [ ] **Step 1: Write the classify pass with tests**

Create `src/classify/pass.rs`:

```rust
use crate::graph::{EdgeKind, Graph, NodeRole};

use super::{ClassifyContext, CompositeClassifier};

/// Run classifiers over all Calls edges in the graph.
/// Enriches edges with direction/operation and marks terminal nodes.
pub fn classify_graph(graph: &mut Graph, classifier: &CompositeClassifier) {
    // Build a set of node IDs for lookup
    let node_files: std::collections::HashMap<&str, &std::path::Path> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.file.as_path()))
        .collect();

    // Collect classifications first (borrow checker)
    let classifications: Vec<(usize, super::Classification)> = graph
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == EdgeKind::Calls)
        .filter_map(|(i, edge)| {
            let file = node_files
                .get(edge.source.as_str())
                .copied()
                .unwrap_or(std::path::Path::new(""));
            let ctx = ClassifyContext {
                source_node: edge.source.clone(),
                file: file.to_path_buf(),
                arguments: vec![],
            };
            let target_name = edge.target.rsplit("::").next().unwrap_or(&edge.target);
            classifier.classify(target_name, &ctx).map(|c| (i, c))
        })
        .collect();

    // Track which nodes are terminals
    let mut terminal_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Apply classifications
    for (idx, classification) in &classifications {
        let edge = &mut graph.edges[*idx];
        edge.direction = Some(classification.direction);
        edge.operation = Some(classification.operation.clone());
        terminal_nodes.insert(edge.target.clone());
    }

    // Mark terminal nodes
    for node in &mut graph.nodes {
        if terminal_nodes.contains(&node.id) {
            // Find the classification for this node
            if let Some((_, classification)) = classifications
                .iter()
                .find(|(idx, _)| graph.edges[*idx].target == node.id)
            {
                node.role = Some(NodeRole::Terminal {
                    kind: classification.terminal_kind,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{Classification, Classifier, ClassifyContext};
    use crate::graph::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    struct MockClassifier;
    impl Classifier for MockClassifier {
        fn classify(&self, call_target: &str, _ctx: &ClassifyContext) -> Option<Classification> {
            if call_target == "save" {
                Some(Classification {
                    terminal_kind: TerminalKind::Persistence,
                    direction: FlowDirection::Write,
                    operation: "save".to_string(),
                })
            } else {
                None
            }
        }
    }

    fn make_node(id: &str, name: &str) -> Node {
        Node {
            id: id.to_string(),
            kind: NodeKind::Function,
            name: name.to_string(),
            file: PathBuf::from("test.rs"),
            span: Span { start: [0, 0], end: [1, 0] },
            visibility: Visibility::Public,
            metadata: HashMap::new(),
            role: None,
            signature: None,
            doc_comment: None,
            module: None,
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.to_string(),
            target: target.to_string(),
            kind: EdgeKind::Calls,
            confidence: 0.9,
            direction: None,
            operation: None,
            condition: None,
            async_boundary: None,
        }
    }

    #[test]
    fn enriches_matching_edges() {
        let mut graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("main", "main"),
                make_node("db::save", "save"),
            ],
            edges: vec![make_edge("main", "db::save")],
        };

        let classifier = CompositeClassifier::new(vec![Box::new(MockClassifier)]);
        classify_graph(&mut graph, &classifier);

        assert_eq!(graph.edges[0].direction, Some(FlowDirection::Write));
        assert_eq!(graph.edges[0].operation.as_deref(), Some("save"));
    }

    #[test]
    fn marks_terminal_nodes() {
        let mut graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("main", "main"),
                make_node("db::save", "save"),
            ],
            edges: vec![make_edge("main", "db::save")],
        };

        let classifier = CompositeClassifier::new(vec![Box::new(MockClassifier)]);
        classify_graph(&mut graph, &classifier);

        assert!(graph.nodes[0].role.is_none()); // main is not terminal
        assert_eq!(
            graph.nodes[1].role,
            Some(NodeRole::Terminal { kind: TerminalKind::Persistence })
        );
    }

    #[test]
    fn skips_non_calls_edges() {
        let mut graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("a", "a"),
                make_node("b::save", "save"),
            ],
            edges: vec![Edge {
                source: "a".to_string(),
                target: "b::save".to_string(),
                kind: EdgeKind::TypeRef,
                confidence: 0.9,
                direction: None,
                operation: None,
                condition: None,
                async_boundary: None,
            }],
        };

        let classifier = CompositeClassifier::new(vec![Box::new(MockClassifier)]);
        classify_graph(&mut graph, &classifier);

        assert!(graph.edges[0].direction.is_none());
    }
}
```

- [ ] **Step 2: Register module**

Add `pub mod pass;` to `src/classify.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib classify::pass::tests`
Expected: All 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/classify/pass.rs src/classify.rs
git commit -m "feat: add classify post-pass to enrich graph edges"
```

---

## Phase 4: Extraction Enhancements

### Task 9: Extract conditions and async boundaries in Rust extractor

**Files:**
- Modify: `src/extract/rust.rs`

This task adds condition context and async boundary detection to the Rust tree-sitter extraction. When a function call is found inside an `if`, `match`, or `if let` block, the condition text is captured on the edge. When `.await` or `tokio::spawn` is detected, `async_boundary` is set.

- [ ] **Step 1: Write fixture file for testing**

Create `tests/fixtures/dataflow_rust.rs`:

```rust
fn main() {
    let config = load_config();
    if config.is_valid() {
        save_to_db(config);
    }
    let result = fetch_data().await;
    match result {
        Ok(data) => process(data),
        Err(e) => log_error(e),
    }
}

fn load_config() -> Config {
    std::fs::read_to_string("config.toml")
}

fn save_to_db(config: Config) {
    db.execute("INSERT INTO configs VALUES (?)", &[config]);
}

async fn fetch_data() -> Result<Data, Error> {
    reqwest::get("https://api.example.com").await
}
```

- [ ] **Step 2: Write test for condition extraction**

Add to `src/extract/rust.rs` tests:

```rust
#[test]
fn extracts_condition_on_call_inside_if() {
    let source = br#"
fn main() {
    if config.is_valid() {
        save_to_db(config);
    }
}
"#;
    let extractor = RustExtractor;
    let result = extractor.extract(source, Path::new("test.rs")).unwrap();
    let save_edge = result.edges.iter().find(|e| e.target.contains("save_to_db"));
    assert!(save_edge.is_some(), "should find save_to_db call edge");
    let edge = save_edge.unwrap();
    assert!(edge.condition.is_some(), "call inside if should have condition");
    assert!(edge.condition.as_ref().unwrap().contains("is_valid"));
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib extract::rust::tests::extracts_condition_on_call_inside_if 2>&1 | head -20`
Expected: FAIL — condition is None

- [ ] **Step 4: Implement condition extraction**

In `src/extract/rust.rs`, modify the function call extraction logic. When emitting a `Calls` edge, walk up from the current node to check for enclosing `if_expression`, `match_expression`, or `if_let_expression`:

```rust
/// Walk up from `node` to find an enclosing conditional, returning its condition text.
fn find_enclosing_condition(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "if_expression" | "if_let_expression" => {
                // The condition is the first child that is not the block
                if let Some(condition_node) = parent.child_by_field_name("condition") {
                    let text = condition_node.utf8_text(source).ok()?;
                    return Some(text.to_string());
                }
                // Fallback: extract text between "if" and "{"
                let full = parent.utf8_text(source).ok()?;
                let cond = full.strip_prefix("if ")?.split('{').next()?.trim();
                if !cond.is_empty() {
                    return Some(cond.to_string());
                }
            }
            "match_expression" => {
                // We're inside a match arm — find which arm
                // Walk back down to find the match_arm pattern
            }
            "match_arm" => {
                if let Some(pattern) = parent.child_by_field_name("pattern") {
                    let text = pattern.utf8_text(source).ok()?;
                    return Some(format!("match {text}"));
                }
            }
            "function_item" | "closure_expression" => {
                break; // Don't cross function boundaries
            }
            _ => {}
        }
        current = parent.parent();
    }
    None
}
```

Then in the call extraction code path, capture the condition:

```rust
// When emitting a Calls edge:
let condition = find_enclosing_condition(call_node, source);
// ... set edge.condition = condition;
```

- [ ] **Step 5: Run the condition test**

Run: `cargo test --lib extract::rust::tests::extracts_condition_on_call_inside_if`
Expected: PASS

- [ ] **Step 6: Write test for async boundary detection**

```rust
#[test]
fn detects_await_as_async_boundary() {
    let source = br#"
async fn main() {
    let result = fetch_data().await;
}
"#;
    let extractor = RustExtractor;
    let result = extractor.extract(source, Path::new("test.rs")).unwrap();
    let fetch_edge = result.edges.iter().find(|e| e.target.contains("fetch_data"));
    assert!(fetch_edge.is_some());
    assert_eq!(fetch_edge.unwrap().async_boundary, Some(true));
}
```

- [ ] **Step 7: Implement async boundary detection**

In the call extraction path, check if the call expression is followed by `.await` or is inside `tokio::spawn`:

```rust
/// Check if a call expression has an .await suffix or is inside a spawn block.
fn is_async_boundary(call_node: tree_sitter::Node, source: &[u8]) -> bool {
    // Check for .await on the call expression
    if let Some(parent) = call_node.parent() {
        if parent.kind() == "await_expression" {
            return true;
        }
    }
    // Check if inside tokio::spawn or std::thread::spawn
    let mut current = call_node.parent();
    while let Some(parent) = current {
        if parent.kind() == "call_expression" {
            let text = parent.utf8_text(source).unwrap_or("");
            if text.starts_with("tokio::spawn") || text.starts_with("std::thread::spawn") {
                return true;
            }
        }
        if parent.kind() == "function_item" {
            break;
        }
        current = parent.parent();
    }
    false
}
```

- [ ] **Step 8: Run all Rust extractor tests**

Run: `cargo test --lib extract::rust::tests`
Expected: All PASS

- [ ] **Step 9: Commit**

```bash
git add src/extract/rust.rs tests/fixtures/dataflow_rust.rs
git commit -m "feat: extract conditions and async boundaries in Rust extractor"
```

### Task 10: Extract conditions, async boundaries, and signatures in Swift extractor

**Files:**
- Modify: `src/extract/swift.rs`
- Create: `tests/fixtures/dataflow_swift.swift`

- [ ] **Step 1: Create Swift fixture**

Create `tests/fixtures/dataflow_swift.swift`:

```swift
import SwiftUI

struct ContentView: View {
    @StateObject var viewModel = UserViewModel()

    var body: some View {
        Button("Save") {
            viewModel.saveProfile()
        }
        .onAppear {
            viewModel.loadUser()
        }
    }
}

@Observable
class UserViewModel {
    func loadUser() {
        guard let userId = currentUserId else { return }
        Task {
            let user = await apiClient.fetchUser(id: userId)
            self.user = user
        }
    }

    func saveProfile() {
        if formState.isValid {
            coreDataStack.save(user)
            NotificationCenter.default.post(name: .profileUpdated)
        }
    }
}
```

- [ ] **Step 2: Write test for Swift condition extraction**

Add to `src/extract/swift.rs` tests:

```rust
#[test]
fn extracts_condition_on_call_inside_if() {
    let source = br#"
class Foo {
    func bar() {
        if isValid {
            save(data)
        }
    }
}
"#;
    let extractor = SwiftExtractor;
    let result = extractor.extract(source, Path::new("test.swift")).unwrap();
    let save_edge = result.edges.iter().find(|e| e.target.contains("save"));
    assert!(save_edge.is_some());
    let edge = save_edge.unwrap();
    assert!(edge.condition.is_some());
    assert!(edge.condition.as_ref().unwrap().contains("isValid"));
}
```

- [ ] **Step 3: Implement condition extraction for Swift**

Same approach as Rust — walk up from call node looking for `if_statement`, `guard_statement`, `switch_case`:

```rust
fn find_enclosing_condition(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "if_statement" => {
                if let Some(cond) = parent.child_by_field_name("condition") {
                    return Some(cond.utf8_text(source).ok()?.to_string());
                }
            }
            "guard_statement" => {
                if let Some(cond) = parent.child_by_field_name("condition") {
                    return Some(format!("guard {}", cond.utf8_text(source).ok()?));
                }
            }
            "switch_case" => {
                if let Some(pattern) = parent.child(0) {
                    return Some(format!("case {}", pattern.utf8_text(source).ok()?));
                }
            }
            "function_declaration" | "closure_expression" => break,
            _ => {}
        }
        current = parent.parent();
    }
    None
}
```

- [ ] **Step 4: Implement async boundary detection for Swift**

```rust
fn is_async_boundary(call_node: tree_sitter::Node, source: &[u8]) -> bool {
    // Check for await keyword
    if let Some(parent) = call_node.parent() {
        if parent.kind() == "await_expression" {
            return true;
        }
    }
    // Check if inside Task { } or DispatchQueue.async
    let mut current = call_node.parent();
    while let Some(parent) = current {
        let text = parent.utf8_text(source).unwrap_or("");
        if parent.kind() == "call_expression" && (text.starts_with("Task") || text.contains("DispatchQueue") && text.contains("async")) {
            return true;
        }
        if parent.kind() == "function_declaration" {
            break;
        }
        current = parent.parent();
    }
    false
}
```

- [ ] **Step 5: Extract function signatures**

When extracting functions, capture the signature text:

```rust
// In function extraction, after getting the name:
let signature = {
    let full_text = node.utf8_text(source).unwrap_or("");
    // Take everything up to the opening brace
    full_text.split('{').next().map(|s| s.trim().to_string())
};
// Set node.signature = signature;
```

- [ ] **Step 6: Extract doc comments**

When extracting any declaration, check the previous sibling for comment nodes:

```rust
fn extract_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut prev = node.prev_sibling();
    let mut comments = Vec::new();
    while let Some(sibling) = prev {
        if sibling.kind() == "comment" || sibling.kind() == "multiline_comment" {
            comments.push(sibling.utf8_text(source).ok()?.to_string());
            prev = sibling.prev_sibling();
        } else {
            break;
        }
    }
    if comments.is_empty() {
        None
    } else {
        comments.reverse();
        Some(comments.join("\n"))
    }
}
```

- [ ] **Step 7: Run all Swift extractor tests**

Run: `cargo test --lib extract::swift::tests`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add src/extract/swift.rs tests/fixtures/dataflow_swift.swift
git commit -m "feat: extract conditions, async boundaries, signatures in Swift extractor"
```

### Task 11: Entry point auto-detection in extractors

**Files:**
- Modify: `src/extract/swift.rs`
- Modify: `src/extract/rust.rs`

- [ ] **Step 1: Write test for Swift entry point detection**

```rust
#[test]
fn detects_view_conformance_as_entry_point() {
    let source = br#"
struct ContentView: View {
    var body: some View {
        Text("Hello")
    }
}
"#;
    let extractor = SwiftExtractor;
    let result = extractor.extract(source, Path::new("test.swift")).unwrap();
    let body_node = result.nodes.iter().find(|n| n.name == "body");
    assert!(body_node.is_some());
    assert_eq!(body_node.unwrap().role, Some(NodeRole::EntryPoint));
}
```

- [ ] **Step 2: Implement Swift entry point detection**

During extraction, when we encounter:
- A struct/class inheriting `View` → mark `body` property as `EntryPoint`
- A struct with `@main` → mark as `EntryPoint`
- A class with `@Observable` or conforming to `ObservableObject` → mark public methods as `EntryPoint`

In the Swift extractor's struct/class extraction, check the inheritance clause for "View", "App", "ObservableObject". When found, tag the appropriate child nodes.

- [ ] **Step 3: Write test for Rust entry point detection**

```rust
#[test]
fn detects_main_as_entry_point() {
    let source = br#"
fn main() {
    println!("hello");
}
"#;
    let extractor = RustExtractor;
    let result = extractor.extract(source, Path::new("test.rs")).unwrap();
    let main_node = result.nodes.iter().find(|n| n.name == "main");
    assert!(main_node.is_some());
    assert_eq!(main_node.unwrap().role, Some(NodeRole::EntryPoint));
}

#[test]
fn detects_test_as_entry_point() {
    let source = br#"
#[test]
fn test_something() {
    assert!(true);
}
"#;
    let extractor = RustExtractor;
    let result = extractor.extract(source, Path::new("test.rs")).unwrap();
    let test_node = result.nodes.iter().find(|n| n.name == "test_something");
    assert!(test_node.is_some());
    assert_eq!(test_node.unwrap().role, Some(NodeRole::EntryPoint));
}
```

- [ ] **Step 4: Implement Rust entry point detection**

During function extraction:
- `fn main()` at module level → `EntryPoint`
- Function with `#[test]` attribute → `EntryPoint`
- Function with `#[tokio::main]` → `EntryPoint`
- `pub fn` at crate root → `EntryPoint`

Check attributes by looking at the previous sibling `attribute_item` nodes.

- [ ] **Step 5: Run all extractor tests**

Run: `cargo test --lib extract`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/extract/rust.rs src/extract/swift.rs
git commit -m "feat: auto-detect entry points in Rust and Swift extractors"
```

---

## Phase 5: Module Discovery and Enhanced Merge

### Task 12: Module map discovery

**Files:**
- Create: `src/module.rs`

- [ ] **Step 1: Write module discovery with tests**

Create `src/module.rs`:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Maps module/package name → list of source directories.
#[derive(Debug, Clone, Default)]
pub struct ModuleMap {
    pub modules: HashMap<String, Vec<PathBuf>>,
}

impl ModuleMap {
    /// Build a module map by scanning for Package.swift and Cargo.toml files.
    pub fn discover(root: &Path) -> Self {
        let mut modules = HashMap::new();

        // Scan for Swift packages
        discover_swift_packages(root, &mut modules);

        // Scan for Cargo workspace members
        discover_cargo_workspace(root, &mut modules);

        // If no packages found, treat root as a single module
        if modules.is_empty() {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "root".to_string());
            modules.insert(name, vec![root.to_path_buf()]);
        }

        Self { modules }
    }

    /// Given a file path, determine which module it belongs to.
    pub fn module_for_file(&self, file: &Path) -> Option<String> {
        for (name, dirs) in &self.modules {
            for dir in dirs {
                if file.starts_with(dir) {
                    return Some(name.clone());
                }
            }
        }
        None
    }
}

fn discover_swift_packages(root: &Path, modules: &mut HashMap<String, Vec<PathBuf>>) {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .max_depth(Some(4))
        .build();

    for entry in walker.flatten() {
        if entry.file_name().to_string_lossy() == "Package.swift" {
            let pkg_dir = entry.path().parent().unwrap_or(root);
            let name = pkg_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let sources_dir = pkg_dir.join("Sources");
            if sources_dir.exists() {
                modules.insert(name, vec![sources_dir]);
            } else {
                modules.insert(name, vec![pkg_dir.to_path_buf()]);
            }
        }
    }
}

fn discover_cargo_workspace(root: &Path, modules: &mut HashMap<String, Vec<PathBuf>>) {
    let cargo_path = root.join("Cargo.toml");
    if !cargo_path.exists() {
        return;
    }
    let contents = match std::fs::read_to_string(&cargo_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Simple parsing: look for [workspace] members
    if !contents.contains("[workspace]") {
        // Single crate
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        modules.insert(name, vec![root.join("src")]);
        return;
    }

    // Parse with toml crate for workspace members
    if let Ok(parsed) = contents.parse::<toml::Table>() {
        if let Some(workspace) = parsed.get("workspace").and_then(|w| w.as_table()) {
            if let Some(members) = workspace.get("members").and_then(|m| m.as_array()) {
                for member in members {
                    if let Some(member_str) = member.as_str() {
                        // Handle glob patterns like "crates/*"
                        if member_str.contains('*') {
                            let base = root.join(member_str.replace("/*", ""));
                            if base.is_dir() {
                                if let Ok(entries) = std::fs::read_dir(&base) {
                                    for entry in entries.flatten() {
                                        if entry.path().is_dir() {
                                            let name = entry.file_name().to_string_lossy().to_string();
                                            modules.insert(name, vec![entry.path().join("src")]);
                                        }
                                    }
                                }
                            }
                        } else {
                            let member_path = root.join(member_str);
                            let name = member_path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            modules.insert(name, vec![member_path.join("src")]);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_swift_package() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("MyPackage");
        let sources = pkg.join("Sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(pkg.join("Package.swift"), "// swift-tools-version: 5.9").unwrap();

        let map = ModuleMap::discover(dir.path());
        assert!(map.modules.contains_key("MyPackage"));
    }

    #[test]
    fn discovers_cargo_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["crate_a", "crate_b"]
"#,
        )
        .unwrap();
        let crate_a = dir.path().join("crate_a/src");
        let crate_b = dir.path().join("crate_b/src");
        std::fs::create_dir_all(&crate_a).unwrap();
        std::fs::create_dir_all(&crate_b).unwrap();

        let map = ModuleMap::discover(dir.path());
        assert!(map.modules.contains_key("crate_a"));
        assert!(map.modules.contains_key("crate_b"));
    }

    #[test]
    fn module_for_file_finds_correct_module() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("MyPkg");
        let sources = pkg.join("Sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(pkg.join("Package.swift"), "// swift").unwrap();

        let map = ModuleMap::discover(dir.path());
        let result = map.module_for_file(&sources.join("Foo.swift"));
        assert_eq!(result.as_deref(), Some("MyPkg"));
    }

    #[test]
    fn falls_back_to_root_module() {
        let dir = tempfile::tempdir().unwrap();
        let map = ModuleMap::discover(dir.path());
        assert_eq!(map.modules.len(), 1);
    }
}
```

- [ ] **Step 2: Register module in `src/main.rs`**

Add `mod module;` to the module declarations.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib module::tests`
Expected: All 4 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/module.rs src/main.rs
git commit -m "feat: add module map discovery for Swift packages and Cargo workspaces"
```

### Task 13: Module-aware merge with enhanced confidence

**Files:**
- Modify: `src/merge.rs`

- [ ] **Step 1: Write test for module-aware resolution**

Add to `src/merge.rs` tests:

```rust
#[test]
fn cross_module_resolution_gets_lower_confidence() {
    let r1 = ExtractionResult {
        nodes: vec![{
            let mut n = make_node("mod_a/src/main.rs::main", "main", NodeKind::Function);
            n.module = Some("mod_a".to_string());
            n
        }],
        edges: vec![Edge {
            source: "mod_a/src/main.rs::main".to_string(),
            target: "mod_a/src/main.rs::helper".to_string(),
            kind: EdgeKind::Calls,
            confidence: 1.0,
            direction: None,
            operation: None,
            condition: None,
            async_boundary: None,
        }],
        imports: vec![],
    };
    let r2 = ExtractionResult {
        nodes: vec![{
            let mut n = make_node("mod_b/src/lib.rs::helper", "helper", NodeKind::Function);
            n.module = Some("mod_b".to_string());
            n
        }],
        edges: vec![],
        imports: vec![],
    };
    let graph = merge(vec![r1, r2]);
    let edge = graph.edges.iter().find(|e| e.kind == EdgeKind::Calls).unwrap();
    // Cross-module resolution: 0.8 confidence
    assert!(edge.confidence <= 0.8);
}
```

- [ ] **Step 2: Update `make_node` test helper**

Update the `make_node` helper in merge tests to include the new fields:

```rust
fn make_node(id: &str, name: &str, kind: NodeKind) -> Node {
    Node {
        id: id.to_string(),
        kind,
        name: name.to_string(),
        file: PathBuf::from("test.rs"),
        span: Span { start: [0, 0], end: [0, 0] },
        visibility: Visibility::Public,
        metadata: HashMap::new(),
        role: None,
        signature: None,
        doc_comment: None,
        module: None,
    }
}
```

- [ ] **Step 3: Implement module-aware merge**

Update `merge()` to use module information for confidence scoring:

```rust
pub fn merge(results: Vec<ExtractionResult>) -> Graph {
    let mut graph = Graph::new();

    for r in &results {
        graph.nodes.extend(r.nodes.iter().cloned());
    }

    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();

    // Build name → vec of (node ID, module) for cross-file lookup
    let mut name_to_entries: HashMap<&str, Vec<(&str, Option<&str>)>> = HashMap::new();
    for node in &graph.nodes {
        name_to_entries
            .entry(node.name.as_str())
            .or_default()
            .push((node.id.as_str(), node.module.as_deref()));
    }

    // Build node ID → module for source lookup
    let id_to_module: HashMap<&str, Option<&str>> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.module.as_deref()))
        .collect();

    for r in results {
        for mut edge in r.edges {
            if node_ids.contains(edge.target.as_str()) || edge.kind == EdgeKind::Uses {
                graph.edges.push(edge);
            } else {
                let target_name = edge.target.rsplit("::").next().unwrap_or(&edge.target);
                if let Some(candidates) = name_to_entries.get(target_name) {
                    let source_module = id_to_module.get(edge.source.as_str()).copied().flatten();

                    if candidates.len() == 1 {
                        edge.target = candidates[0].0.to_string();
                        let target_module = candidates[0].1;
                        // Same module: 0.9, cross-module: 0.8
                        let factor = if source_module == target_module { 0.9 } else { 0.8 };
                        edge.confidence *= factor;
                        graph.edges.push(edge);
                    } else if candidates.len() > 1 {
                        // Prefer same-module candidate
                        let best = candidates
                            .iter()
                            .find(|(_, m)| *m == source_module)
                            .unwrap_or(&candidates[0]);
                        edge.target = best.0.to_string();
                        let factor = if best.1 == source_module { 0.7 } else { 0.5 };
                        edge.confidence *= factor;
                        graph.edges.push(edge);
                    }
                }
            }
        }
    }

    graph
}
```

- [ ] **Step 4: Run all merge tests**

Run: `cargo test --lib merge::tests`
Expected: All PASS (existing tests should still pass since module is None in their nodes)

- [ ] **Step 5: Commit**

```bash
git add src/merge.rs
git commit -m "feat: module-aware cross-file resolution with tiered confidence"
```

---

## Phase 6: Trace and Reverse Queries

### Task 14: Forward dataflow trace

**Files:**
- Create: `src/query/trace.rs`
- Modify: `src/query.rs`

- [ ] **Step 1: Write trace types and implementation**

Create `src/query/trace.rs`:

```rust
use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::graph::{EdgeKind, FlowDirection, Graph, NodeRole};

#[derive(Debug, Serialize)]
pub struct TraceResult {
    pub entry: String,
    pub flows: Vec<Flow>,
    pub summary: TraceSummary,
}

#[derive(Debug, Serialize)]
pub struct Flow {
    pub path: Vec<String>,
    pub terminal: Option<TerminalInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub async_boundaries: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TerminalInfo {
    pub kind: String,
    pub operation: String,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct TraceSummary {
    pub total_flows: usize,
    pub reads: usize,
    pub writes: usize,
    pub async_crossings: usize,
}

/// Forward-trace from an entry point to all reachable terminal operations.
pub fn query_trace(graph: &Graph, entry: &str, max_depth: usize) -> Option<TraceResult> {
    let entry_node = graph
        .nodes
        .iter()
        .find(|n| n.id == entry || n.name == entry)?;

    // Build forward adjacency: source → [(target, edge_index)]
    let mut forward_adj: HashMap<&str, Vec<(&str, usize)>> = HashMap::new();
    for (i, edge) in graph.edges.iter().enumerate() {
        if matches!(
            edge.kind,
            EdgeKind::Calls | EdgeKind::Reads | EdgeKind::Writes | EdgeKind::Publishes | EdgeKind::Subscribes
        ) {
            forward_adj
                .entry(&edge.source)
                .or_default()
                .push((&edge.target, i));
        }
    }

    // Node ID → Node for lookups
    let node_index: HashMap<&str, &crate::graph::Node> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // DFS with path tracking
    let mut flows = Vec::new();
    let mut visited = HashSet::new();

    struct StackEntry<'a> {
        node_id: &'a str,
        path: Vec<String>,
        conditions: Vec<String>,
        async_boundaries: Vec<String>,
        depth: usize,
    }

    let mut stack = vec![StackEntry {
        node_id: &entry_node.id,
        path: vec![entry_node.name.clone()],
        conditions: vec![],
        async_boundaries: vec![],
        depth: 0,
    }];

    while let Some(current) = stack.pop() {
        if current.depth >= max_depth {
            continue;
        }

        if let Some(neighbors) = forward_adj.get(current.node_id) {
            for (target_id, edge_idx) in neighbors {
                if visited.contains(&(current.node_id, *target_id)) {
                    continue;
                }
                visited.insert((current.node_id, *target_id));

                let edge = &graph.edges[*edge_idx];
                let target_name = node_index
                    .get(target_id)
                    .map(|n| n.name.as_str())
                    .unwrap_or(target_id);

                let mut path = current.path.clone();
                path.push(target_name.to_string());

                let mut conditions = current.conditions.clone();
                if let Some(ref cond) = edge.condition {
                    conditions.push(cond.clone());
                }

                let mut async_boundaries = current.async_boundaries.clone();
                if edge.async_boundary == Some(true) {
                    async_boundaries.push(format!(
                        "{} → {}",
                        current.path.last().unwrap_or(&String::new()),
                        target_name
                    ));
                }

                // Check if target is a terminal
                let is_terminal = node_index
                    .get(target_id)
                    .map(|n| matches!(n.role, Some(NodeRole::Terminal { .. })))
                    .unwrap_or(false);

                if is_terminal {
                    let terminal_info = edge.direction.map(|dir| TerminalInfo {
                        kind: node_index
                            .get(target_id)
                            .and_then(|n| match &n.role {
                                Some(NodeRole::Terminal { kind }) => {
                                    serde_json::to_string(kind).ok()
                                }
                                _ => None,
                            })
                            .unwrap_or_else(|| "\"unknown\"".to_string())
                            .trim_matches('"')
                            .to_string(),
                        operation: edge.operation.clone().unwrap_or_default(),
                        direction: format!("{:?}", dir).to_lowercase(),
                    });

                    flows.push(Flow {
                        path,
                        terminal: terminal_info,
                        conditions,
                        async_boundaries,
                    });
                } else {
                    stack.push(StackEntry {
                        node_id: target_id,
                        path,
                        conditions,
                        async_boundaries,
                        depth: current.depth + 1,
                    });
                }
            }
        }
    }

    let reads = flows
        .iter()
        .filter(|f| {
            f.terminal
                .as_ref()
                .map(|t| t.direction == "read")
                .unwrap_or(false)
        })
        .count();
    let writes = flows
        .iter()
        .filter(|f| {
            f.terminal
                .as_ref()
                .map(|t| t.direction == "write")
                .unwrap_or(false)
        })
        .count();
    let async_crossings = flows.iter().filter(|f| !f.async_boundaries.is_empty()).count();

    Some(TraceResult {
        entry: entry_node.id.clone(),
        flows: flows.clone(),
        summary: TraceSummary {
            total_flows: flows.len(),
            reads,
            writes,
            async_crossings,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_node(id: &str, name: &str, role: Option<NodeRole>) -> Node {
        Node {
            id: id.to_string(),
            kind: NodeKind::Function,
            name: name.to_string(),
            file: PathBuf::from("test.rs"),
            span: Span { start: [0, 0], end: [1, 0] },
            visibility: Visibility::Public,
            metadata: HashMap::new(),
            role,
            signature: None,
            doc_comment: None,
            module: None,
        }
    }

    fn make_edge(source: &str, target: &str, direction: Option<FlowDirection>, operation: Option<&str>, condition: Option<&str>) -> Edge {
        Edge {
            source: source.to_string(),
            target: target.to_string(),
            kind: EdgeKind::Calls,
            confidence: 0.9,
            direction,
            operation: operation.map(|s| s.to_string()),
            condition: condition.map(|s| s.to_string()),
            async_boundary: None,
        }
    }

    #[test]
    fn traces_entry_to_terminal() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("main", "main", Some(NodeRole::EntryPoint)),
                make_node("service", "service", None),
                make_node("db_save", "db_save", Some(NodeRole::Terminal { kind: TerminalKind::Persistence })),
            ],
            edges: vec![
                make_edge("main", "service", None, None, None),
                make_edge("service", "db_save", Some(FlowDirection::Write), Some("save"), None),
            ],
        };

        let result = query_trace(&graph, "main", 10).unwrap();
        assert_eq!(result.flows.len(), 1);
        assert_eq!(result.flows[0].path, vec!["main", "service", "db_save"]);
        assert_eq!(result.summary.writes, 1);
    }

    #[test]
    fn captures_conditions() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("main", "main", Some(NodeRole::EntryPoint)),
                make_node("save", "save", Some(NodeRole::Terminal { kind: TerminalKind::Persistence })),
            ],
            edges: vec![
                make_edge("main", "save", Some(FlowDirection::Write), Some("save"), Some("is_valid")),
            ],
        };

        let result = query_trace(&graph, "main", 10).unwrap();
        assert_eq!(result.flows[0].conditions, vec!["is_valid"]);
    }

    #[test]
    fn returns_none_for_unknown_entry() {
        let graph = Graph::new();
        assert!(query_trace(&graph, "nonexistent", 10).is_none());
    }
}
```

- [ ] **Step 2: Register module in `src/query.rs`**

Add `pub mod trace;` to `src/query.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib query::trace::tests`
Expected: All 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/query/trace.rs src/query.rs
git commit -m "feat: add forward dataflow trace query"
```

### Task 15: Reverse query (symbol → affected entry points)

**Files:**
- Create: `src/query/reverse.rs`
- Modify: `src/query.rs`

- [ ] **Step 1: Write reverse query with tests**

Create `src/query/reverse.rs`:

```rust
use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::graph::{EdgeKind, Graph, NodeRole};

use super::SymbolRef;

#[derive(Debug, Serialize)]
pub struct ReverseResult {
    pub symbol: String,
    pub affected_entries: Vec<AffectedEntry>,
    pub total_entries: usize,
}

#[derive(Debug, Serialize)]
pub struct AffectedEntry {
    pub entry: SymbolRef,
    pub distance: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
}

/// Walk backward from a symbol to find all entry points that depend on it.
pub fn query_reverse(graph: &Graph, symbol: &str) -> Option<ReverseResult> {
    let target_node = graph
        .nodes
        .iter()
        .find(|n| n.id == symbol || n.name == symbol)?;

    let node_index: HashMap<&str, &crate::graph::Node> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Build reverse adjacency
    let mut reverse_adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if matches!(
            edge.kind,
            EdgeKind::Calls | EdgeKind::Reads | EdgeKind::Writes | EdgeKind::Publishes | EdgeKind::Subscribes
        ) {
            reverse_adj
                .entry(&edge.target)
                .or_default()
                .push(&edge.source);
        }
    }

    let mut visited: HashSet<&str> = HashSet::new();
    visited.insert(&target_node.id);

    let mut affected_entries = Vec::new();

    // BFS backward, tracking path
    let mut queue: VecDeque<(&str, usize, Vec<String>)> = VecDeque::new();
    queue.push_back((&target_node.id, 0, vec![target_node.name.clone()]));

    while let Some((current, depth, path)) = queue.pop_front() {
        if let Some(callers) = reverse_adj.get(current) {
            for caller_id in callers {
                if visited.contains(caller_id) {
                    continue;
                }
                visited.insert(caller_id);

                if let Some(caller_node) = node_index.get(caller_id) {
                    let mut new_path = path.clone();
                    new_path.push(caller_node.name.clone());

                    if matches!(caller_node.role, Some(NodeRole::EntryPoint)) {
                        affected_entries.push(AffectedEntry {
                            entry: SymbolRef {
                                id: caller_node.id.clone(),
                                name: caller_node.name.clone(),
                                kind: caller_node.kind,
                                file: caller_node.file.to_string_lossy().to_string(),
                            },
                            distance: depth + 1,
                            path: new_path.into_iter().rev().collect(),
                        });
                    }

                    queue.push_back((caller_id, depth + 1, new_path));
                }
            }
        }
    }

    let total = affected_entries.len();
    Some(ReverseResult {
        symbol: target_node.id.clone(),
        affected_entries,
        total_entries: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;
    use std::collections::HashMap as StdHashMap;
    use std::path::PathBuf;

    fn make_node(id: &str, name: &str, role: Option<NodeRole>) -> Node {
        Node {
            id: id.to_string(),
            kind: NodeKind::Function,
            name: name.to_string(),
            file: PathBuf::from("test.rs"),
            span: Span { start: [0, 0], end: [1, 0] },
            visibility: Visibility::Public,
            metadata: StdHashMap::new(),
            role,
            signature: None,
            doc_comment: None,
            module: None,
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.to_string(),
            target: target.to_string(),
            kind: EdgeKind::Calls,
            confidence: 0.9,
            direction: None,
            operation: None,
            condition: None,
            async_boundary: None,
        }
    }

    #[test]
    fn finds_entry_points_that_reach_symbol() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("entry1", "entry1", Some(NodeRole::EntryPoint)),
                make_node("middle", "middle", None),
                make_node("target", "target", Some(NodeRole::Terminal { kind: TerminalKind::Persistence })),
            ],
            edges: vec![
                make_edge("entry1", "middle"),
                make_edge("middle", "target"),
            ],
        };

        let result = query_reverse(&graph, "target").unwrap();
        assert_eq!(result.total_entries, 1);
        assert_eq!(result.affected_entries[0].entry.name, "entry1");
        assert_eq!(result.affected_entries[0].distance, 2);
    }

    #[test]
    fn finds_multiple_entry_points() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("entry1", "entry1", Some(NodeRole::EntryPoint)),
                make_node("entry2", "entry2", Some(NodeRole::EntryPoint)),
                make_node("shared", "shared", None),
            ],
            edges: vec![
                make_edge("entry1", "shared"),
                make_edge("entry2", "shared"),
            ],
        };

        let result = query_reverse(&graph, "shared").unwrap();
        assert_eq!(result.total_entries, 2);
    }

    #[test]
    fn returns_none_for_unknown_symbol() {
        let graph = Graph::new();
        assert!(query_reverse(&graph, "nonexistent").is_none());
    }
}
```

- [ ] **Step 2: Register module**

Add `pub mod reverse;` to `src/query.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib query::reverse::tests`
Expected: All 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/query/reverse.rs src/query.rs
git commit -m "feat: add reverse query (symbol → affected entry points)"
```

### Task 16: Entries listing query

**Files:**
- Create: `src/query/entries.rs`
- Modify: `src/query.rs`

- [ ] **Step 1: Write entries query with tests**

Create `src/query/entries.rs`:

```rust
use serde::Serialize;

use crate::graph::{Graph, NodeRole};

use super::SymbolRef;

#[derive(Debug, Serialize)]
pub struct EntriesResult {
    pub entries: Vec<SymbolRef>,
    pub total: usize,
}

/// List all auto-detected entry points in the graph.
pub fn query_entries(graph: &Graph) -> EntriesResult {
    let entries: Vec<SymbolRef> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.role, Some(NodeRole::EntryPoint)))
        .map(|n| SymbolRef {
            id: n.id.clone(),
            name: n.name.clone(),
            kind: n.kind,
            file: n.file.to_string_lossy().to_string(),
        })
        .collect();

    let total = entries.len();
    EntriesResult { entries, total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_node(id: &str, name: &str, role: Option<NodeRole>) -> Node {
        Node {
            id: id.to_string(),
            kind: NodeKind::Function,
            name: name.to_string(),
            file: PathBuf::from("test.rs"),
            span: Span { start: [0, 0], end: [1, 0] },
            visibility: Visibility::Public,
            metadata: HashMap::new(),
            role,
            signature: None,
            doc_comment: None,
            module: None,
        }
    }

    #[test]
    fn lists_entry_points() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                make_node("main", "main", Some(NodeRole::EntryPoint)),
                make_node("helper", "helper", None),
                make_node("test_foo", "test_foo", Some(NodeRole::EntryPoint)),
            ],
            edges: vec![],
        };

        let result = query_entries(&graph);
        assert_eq!(result.total, 2);
        assert!(result.entries.iter().any(|e| e.name == "main"));
        assert!(result.entries.iter().any(|e| e.name == "test_foo"));
    }

    #[test]
    fn returns_empty_when_no_entries() {
        let graph = Graph::new();
        let result = query_entries(&graph);
        assert_eq!(result.total, 0);
    }
}
```

- [ ] **Step 2: Register module**

Add `pub mod entries;` to `src/query.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib query::entries::tests`
Expected: All 2 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/query/entries.rs src/query.rs
git commit -m "feat: add entries listing query"
```

---

## Phase 7: CLI Integration

### Task 17: Wire new subcommands into CLI

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add new subcommands to the `Commands` enum**

```rust
/// Forward-trace dataflow from an entry point to terminal operations
Trace {
    /// Entry point symbol name or ID
    entry: String,
    /// Maximum traversal depth
    #[arg(long, default_value = "10")]
    depth: usize,
    /// Project directory
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
},
/// Reverse query: which entry points are affected by this symbol?
Reverse {
    /// Symbol name or ID
    symbol: String,
    /// Project directory
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
},
/// List auto-detected entry points
Entries {
    /// Project directory
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
},
```

- [ ] **Step 2: Add match arms in `main()`**

```rust
Commands::Trace { entry, depth, path } => {
    let graph = load_graph(&path)?;
    let result = query::trace::query_trace(&graph, &entry, depth)
        .ok_or_else(|| anyhow::anyhow!("entry point not found: {entry}"))?;
    println!("{}", serde_json::to_string_pretty(&result)?);
}
Commands::Reverse { symbol, path } => {
    let graph = load_graph(&path)?;
    let result = query::reverse::query_reverse(&graph, &symbol)
        .ok_or_else(|| anyhow::anyhow!("symbol not found: {symbol}"))?;
    println!("{}", serde_json::to_string_pretty(&result)?);
}
Commands::Entries { path } => {
    let graph = load_graph(&path)?;
    let result = query::entries::query_entries(&graph);
    println!("{}", serde_json::to_string_pretty(&result)?);
}
```

- [ ] **Step 3: Integrate classify pass into the pipeline**

Update `run_pipeline` to accept a config and run classification after merge:

```rust
fn run_pipeline(path: &Path, verbose: bool) -> anyhow::Result<graph::Graph> {
    // ... existing code up to merge ...

    let t = Instant::now();
    let mut graph = merge::merge(results);
    if verbose {
        progress::done(
            &format!("merged → {} nodes, {} edges", graph.nodes.len(), graph.edges.len()),
            t,
        );
    }

    // Classify pass
    let t = Instant::now();
    let config = config::load_config(path);
    let classifiers: Vec<Box<dyn classify::Classifier>> = vec![
        Box::new(classify::toml_rules::TomlClassifier::new(&config.classifiers)),
        Box::new(classify::swift::SwiftClassifier),
        Box::new(classify::rust::RustClassifier),
    ];
    let composite = classify::CompositeClassifier::new(classifiers);
    classify::pass::classify_graph(&mut graph, &composite);
    if verbose {
        let terminal_count = graph.nodes.iter()
            .filter(|n| matches!(n.role, Some(graph::NodeRole::Terminal { .. })))
            .count();
        let entry_count = graph.nodes.iter()
            .filter(|n| matches!(n.role, Some(graph::NodeRole::EntryPoint)))
            .count();
        progress::done(
            &format!("classified → {} entries, {} terminals", entry_count, terminal_count),
            t,
        );
    }

    Ok(graph)
}
```

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: add trace, reverse, entries subcommands and classify pipeline pass"
```

---

## Phase 8: Web UI

### Task 18: Add axum and tokio dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add dependencies**

```toml
axum = "0.8"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tower-http = { version = "0.6", features = ["cors"] }
```

- [ ] **Step 2: Run `cargo build` to verify**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add axum, tokio, tower-http dependencies for web UI"
```

### Task 19: Web UI HTML/JS frontend

**Files:**
- Create: `src/serve/web/index.html`

- [ ] **Step 1: Create the single-page app**

Create `src/serve/web/index.html` — a self-contained HTML file with embedded CSS and JS using vis-network from CDN. This file will be compiled into the binary via `include_str!`.

The HTML should include:
- Header bar with search input, entries dropdown, filter dropdown
- Main canvas area for vis-network graph
- Right sidebar for symbol detail panel
- Color legend
- All vis-network initialization and API fetch logic

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Grapha — Code Graph Explorer</title>
    <script src="https://unpkg.com/vis-network@9.1.6/standalone/umd/vis-network.min.js"></script>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #1a1a2e; color: #eee; }
        #header { display: flex; align-items: center; gap: 12px; padding: 12px 16px; background: #16213e; border-bottom: 1px solid #333; }
        #header input { padding: 6px 12px; border-radius: 4px; border: 1px solid #444; background: #1a1a2e; color: #eee; width: 240px; }
        #header select { padding: 6px 8px; border-radius: 4px; border: 1px solid #444; background: #1a1a2e; color: #eee; }
        #header h1 { font-size: 16px; color: #0f3460; margin-right: auto; }
        #header h1 { color: #e94560; }
        #main { display: flex; height: calc(100vh - 49px); }
        #graph-container { flex: 1; }
        #detail-panel { width: 320px; background: #16213e; border-left: 1px solid #333; padding: 16px; overflow-y: auto; display: none; }
        #detail-panel.active { display: block; }
        #detail-panel h2 { font-size: 14px; color: #e94560; margin-bottom: 8px; }
        #detail-panel .field { margin-bottom: 12px; }
        #detail-panel .label { font-size: 11px; color: #888; text-transform: uppercase; }
        #detail-panel .value { font-size: 13px; margin-top: 2px; }
        #detail-panel .list { list-style: none; padding: 0; }
        #detail-panel .list li { font-size: 12px; padding: 2px 0; cursor: pointer; color: #7ec8e3; }
        #detail-panel .list li:hover { text-decoration: underline; }
        #legend { position: absolute; bottom: 16px; left: 16px; background: rgba(22,33,62,0.9); padding: 12px; border-radius: 6px; font-size: 11px; }
        #legend .item { display: flex; align-items: center; gap: 6px; margin-bottom: 4px; }
        #legend .dot { width: 10px; height: 10px; border-radius: 50%; }
    </style>
</head>
<body>
    <div id="header">
        <h1>Grapha</h1>
        <input id="search" type="text" placeholder="Search symbols...">
        <select id="entries"><option value="">— Entry Points —</option></select>
        <select id="filter">
            <option value="all">All</option>
            <option value="entry_point">Entry Points</option>
            <option value="terminal">Terminals</option>
            <option value="function">Functions</option>
            <option value="struct">Structs</option>
        </select>
    </div>
    <div id="main">
        <div id="graph-container"></div>
        <div id="detail-panel">
            <h2 id="detail-name">—</h2>
            <div class="field"><div class="label">Kind</div><div class="value" id="detail-kind">—</div></div>
            <div class="field"><div class="label">Role</div><div class="value" id="detail-role">—</div></div>
            <div class="field"><div class="label">File</div><div class="value" id="detail-file">—</div></div>
            <div class="field"><div class="label">Signature</div><div class="value" id="detail-sig">—</div></div>
            <div class="field"><div class="label">Module</div><div class="value" id="detail-module">—</div></div>
            <div class="field"><div class="label">Callers</div><ul class="list" id="detail-callers"></ul></div>
            <div class="field"><div class="label">Callees</div><ul class="list" id="detail-callees"></ul></div>
        </div>
    </div>
    <div id="legend">
        <div class="item"><div class="dot" style="background:#4caf50"></div> Entry Point</div>
        <div class="item"><div class="dot" style="background:#f44336"></div> Terminal (Write)</div>
        <div class="item"><div class="dot" style="background:#2196f3"></div> Terminal (Read)</div>
        <div class="item"><div class="dot" style="background:#ff9800"></div> Terminal (Event)</div>
        <div class="item"><div class="dot" style="background:#9e9e9e"></div> Internal</div>
    </div>
    <script>
        let network, allNodes, allEdges, graphData;

        function nodeColor(node) {
            if (node.role === 'entry_point') return '#4caf50';
            if (node.role && node.role.type === 'terminal') {
                const kind = node.role.kind;
                if (kind === 'network' || kind === 'persistence') return '#f44336';
                if (kind === 'event') return '#ff9800';
                return '#2196f3';
            }
            return '#9e9e9e';
        }

        function edgeColor(edge) {
            if (edge.direction === 'write') return '#f44336';
            if (edge.direction === 'read') return '#2196f3';
            return '#666';
        }

        async function loadGraph() {
            const resp = await fetch('/api/graph');
            graphData = await resp.json();

            allNodes = new vis.DataSet(graphData.nodes.map(n => ({
                id: n.id, label: n.name, color: nodeColor(n),
                font: { color: '#eee', size: 11 },
                shape: n.role === 'entry_point' ? 'diamond' : 'dot',
                _data: n
            })));

            allEdges = new vis.DataSet(graphData.edges.map((e, i) => ({
                id: i, from: e.source, to: e.target,
                color: { color: edgeColor(e), opacity: 0.6 },
                arrows: 'to', dashes: e.async_boundary === true,
                _data: e
            })));

            const container = document.getElementById('graph-container');
            network = new vis.Network(container, { nodes: allNodes, edges: allEdges }, {
                layout: { improvedLayout: true },
                physics: { solver: 'forceAtlas2Based', forceAtlas2Based: { gravitationalConstant: -30 } },
                interaction: { hover: true },
                nodes: { borderWidth: 1, size: 16 },
                edges: { smooth: { type: 'continuous' } }
            });

            network.on('click', async (params) => {
                if (params.nodes.length > 0) {
                    const nodeId = params.nodes[0];
                    await showDetail(nodeId);
                }
            });

            await loadEntries();
        }

        async function loadEntries() {
            const resp = await fetch('/api/entries');
            const data = await resp.json();
            const select = document.getElementById('entries');
            data.entries.forEach(e => {
                const opt = document.createElement('option');
                opt.value = e.id;
                opt.textContent = e.name;
                select.appendChild(opt);
            });
        }

        async function showDetail(nodeId) {
            const resp = await fetch(`/api/context/${encodeURIComponent(nodeId)}`);
            const ctx = await resp.json();
            const panel = document.getElementById('detail-panel');
            panel.classList.add('active');
            document.getElementById('detail-name').textContent = ctx.symbol.name;
            document.getElementById('detail-kind').textContent = ctx.symbol.kind;
            document.getElementById('detail-file').textContent = ctx.symbol.file;

            const node = allNodes.get(nodeId);
            const data = node._data;
            document.getElementById('detail-role').textContent = data.role ? JSON.stringify(data.role) : 'internal';
            document.getElementById('detail-sig').textContent = data.signature || '—';
            document.getElementById('detail-module').textContent = data.module || '—';

            const callersList = document.getElementById('detail-callers');
            callersList.innerHTML = ctx.callers.map(c => `<li onclick="network.selectNodes(['${c.id}'])">${c.name}</li>`).join('');
            const calleesList = document.getElementById('detail-callees');
            calleesList.innerHTML = ctx.callees.map(c => `<li onclick="network.selectNodes(['${c.id}'])">${c.name}</li>`).join('');
        }

        document.getElementById('search').addEventListener('input', (e) => {
            const q = e.target.value.toLowerCase();
            if (!q) { allNodes.forEach(n => allNodes.update({ id: n.id, hidden: false })); return; }
            allNodes.forEach(n => {
                allNodes.update({ id: n.id, hidden: !n.label.toLowerCase().includes(q) });
            });
        });

        document.getElementById('entries').addEventListener('change', async (e) => {
            if (!e.target.value) return;
            const resp = await fetch(`/api/trace/${encodeURIComponent(e.target.value)}`);
            const trace = await resp.json();
            // Highlight trace paths
            const nodeIds = new Set();
            trace.flows.forEach(f => f.path.forEach(p => {
                const node = graphData.nodes.find(n => n.name === p);
                if (node) nodeIds.add(node.id);
            }));
            network.selectNodes([...nodeIds]);
        });

        document.getElementById('filter').addEventListener('change', (e) => {
            const val = e.target.value;
            allNodes.forEach(n => {
                let show = true;
                if (val === 'entry_point') show = n._data.role === 'entry_point';
                else if (val === 'terminal') show = n._data.role && n._data.role.type === 'terminal';
                else if (val !== 'all') show = n._data.kind === val;
                allNodes.update({ id: n.id, hidden: !show });
            });
        });

        loadGraph();
    </script>
</body>
</html>
```

- [ ] **Step 2: Commit**

```bash
mkdir -p src/serve/web
git add src/serve/web/index.html
git commit -m "feat: add web UI frontend (vis-network single-page app)"
```

### Task 20: HTTP server and API handlers

**Files:**
- Create: `src/serve.rs`
- Create: `src/serve/api.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the server module**

Create `src/serve.rs`:

```rust
pub mod api;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use axum::response::Html;

use crate::graph::Graph;

const INDEX_HTML: &str = include_str!("serve/web/index.html");

pub struct AppState {
    pub graph: Graph,
}

pub async fn run(graph: Graph, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState { graph });

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/api/graph", get(api::get_graph))
        .route("/api/entries", get(api::get_entries))
        .route("/api/context/{symbol}", get(api::get_context))
        .route("/api/trace/{symbol}", get(api::get_trace))
        .route("/api/reverse/{symbol}", get(api::get_reverse))
        .route("/api/search", get(api::get_search))
        .with_state(state)
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = format!("0.0.0.0:{port}");
    eprintln!("  \x1b[32m✓\x1b[0m serving at http://localhost:{port}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

- [ ] **Step 2: Write the API handlers**

Create `src/serve/api.rs`:

```rust
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::query;
use crate::serve::AppState;

pub async fn get_graph(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&state.graph).unwrap_or_default())
}

pub async fn get_entries(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let result = query::entries::query_entries(&state.graph);
    Json(serde_json::to_value(&result).unwrap_or_default())
}

pub async fn get_context(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let decoded = urlencoding::decode(&symbol).unwrap_or(symbol.into());
    query::context::query_context(&state.graph, &decoded)
        .map(|r| Json(serde_json::to_value(&r).unwrap_or_default()))
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn get_trace(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let decoded = urlencoding::decode(&symbol).unwrap_or(symbol.into());
    query::trace::query_trace(&state.graph, &decoded, 10)
        .map(|r| Json(serde_json::to_value(&r).unwrap_or_default()))
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn get_reverse(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let decoded = urlencoding::decode(&symbol).unwrap_or(symbol.into());
    query::reverse::query_reverse(&state.graph, &decoded)
        .map(|r| Json(serde_json::to_value(&r).unwrap_or_default()))
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize { 20 }

pub async fn get_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Json<serde_json::Value> {
    // Simple in-memory search as fallback (tantivy requires index on disk)
    let results: Vec<_> = state.graph.nodes.iter()
        .filter(|n| n.name.to_lowercase().contains(&params.q.to_lowercase()))
        .take(params.limit)
        .map(|n| serde_json::json!({
            "id": n.id,
            "name": n.name,
            "kind": n.kind,
            "file": n.file.to_string_lossy(),
        }))
        .collect();
    Json(serde_json::json!({ "results": results }))
}
```

- [ ] **Step 3: Add `urlencoding` dependency**

Add to `Cargo.toml`:

```toml
urlencoding = "2"
```

- [ ] **Step 4: Register module and add Serve subcommand**

In `src/main.rs`, add `mod serve;` and the `Serve` variant:

```rust
/// Launch web UI for interactive graph exploration
Serve {
    /// Project directory
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    port: u16,
},
```

And the match arm:

```rust
Commands::Serve { path, port } => {
    let graph = load_graph(&path)?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(serve::run(graph, port))?;
}
```

- [ ] **Step 5: Run `cargo build`**

Run: `cargo build 2>&1 | tail -10`
Expected: Compiles successfully

- [ ] **Step 6: Commit**

```bash
git add src/serve.rs src/serve/api.rs Cargo.toml src/main.rs
git commit -m "feat: add grapha serve web UI with REST API"
```

---

## Phase 9: Integration Testing

### Task 21: End-to-end integration tests

**Files:**
- Create: `tests/dataflow_integration.rs`

- [ ] **Step 1: Write integration test for full pipeline with dataflow**

Create `tests/dataflow_integration.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;
use std::fs;

#[test]
fn analyze_produces_dataflow_fields() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        r#"
fn main() {
    if config.is_valid() {
        save_data();
    }
}

fn save_data() {
    std::fs::write("output.txt", "data").unwrap();
}
"#,
    ).unwrap();

    Command::cargo_bin("grapha")
        .unwrap()
        .args(["analyze", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("nodes"))
        .stdout(predicate::str::contains("edges"));
}

#[test]
fn index_and_entries_works() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        r#"
fn main() {
    helper();
}

fn helper() {}
"#,
    ).unwrap();

    // Index
    Command::cargo_bin("grapha")
        .unwrap()
        .args(["index", dir.path().to_str().unwrap()])
        .assert()
        .success();

    // Entries
    Command::cargo_bin("grapha")
        .unwrap()
        .args(["entries", "-p", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("entries"));
}

#[test]
fn trace_command_works() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        r#"
fn main() {
    process();
}

fn process() {}
"#,
    ).unwrap();

    Command::cargo_bin("grapha")
        .unwrap()
        .args(["index", dir.path().to_str().unwrap()])
        .assert()
        .success();

    Command::cargo_bin("grapha")
        .unwrap()
        .args(["trace", "main", "-p", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("entry"));
}

#[test]
fn reverse_command_works() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        r#"
fn main() {
    helper();
}

fn helper() {}
"#,
    ).unwrap();

    Command::cargo_bin("grapha")
        .unwrap()
        .args(["index", dir.path().to_str().unwrap()])
        .assert()
        .success();

    Command::cargo_bin("grapha")
        .unwrap()
        .args(["reverse", "helper", "-p", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("symbol"));
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test dataflow_integration`
Expected: All PASS

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests PASS (original 79 + all new tests)

- [ ] **Step 4: Commit**

```bash
git add tests/dataflow_integration.rs
git commit -m "test: add end-to-end integration tests for dataflow features"
```

---

## Phase 10: Update Compressed Output

### Task 22: Include dataflow fields in grouped/compact output

**Files:**
- Modify: `src/compress/group.rs`

- [ ] **Step 1: Write test for dataflow fields in grouped output**

```rust
#[test]
fn grouped_output_includes_role_and_direction() {
    use crate::graph::{NodeRole, TerminalKind, FlowDirection};

    let graph = Graph {
        version: "0.1.0".to_string(),
        nodes: vec![{
            let mut n = make_node("a.rs::fetch", "fetch", NodeKind::Function, "a.rs", 0);
            n.role = Some(NodeRole::Terminal { kind: TerminalKind::Network });
            n.signature = Some("fn fetch(url: &str)".to_string());
            n
        }],
        edges: vec![],
    };
    let grouped = group(&graph);
    let json = serde_json::to_string(&grouped).unwrap();
    assert!(json.contains("terminal"));
    assert!(json.contains("signature"));
}
```

- [ ] **Step 2: Extend `SymbolSummary` with new fields**

```rust
#[derive(Debug, Serialize)]
pub struct SymbolSummary {
    pub name: String,
    pub kind: NodeKind,
    pub span: [usize; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<crate::graph::NodeRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inherits: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub type_refs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
}
```

- [ ] **Step 3: Update `group()` to populate new fields**

In the `group()` function, populate the new fields from node data and handle new edge kinds:

```rust
// Add to the match in the edge processing loop:
EdgeKind::Reads => reads.push(target_name.to_string()),
EdgeKind::Writes => writes.push(target_name.to_string()),
EdgeKind::Publishes => writes.push(format!("publish:{}", target_name)),
EdgeKind::Subscribes => reads.push(format!("subscribe:{}", target_name)),
```

And when constructing `SymbolSummary`:

```rust
SymbolSummary {
    name: node.name.clone(),
    kind: node.kind,
    span: [node.span.start[0], node.span.end[0]],
    role: node.role.clone(),
    signature: node.signature.clone(),
    module: node.module.clone(),
    members,
    calls,
    implements,
    inherits,
    type_refs,
    reads,
    writes,
}
```

- [ ] **Step 4: Run all compress tests**

Run: `cargo test --lib compress`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/compress/group.rs
git commit -m "feat: include dataflow fields in compact grouped output"
```

---

## Phase 11: Documentation Update

### Task 23: Update CLAUDE.md with new commands and architecture

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update CLI subcommands section**

Add to the CLI subcommands table:

```markdown
grapha trace <entry> [--depth N] [-p PATH]
grapha reverse <symbol> [-p PATH]
grapha entries [-p PATH]
grapha serve [-p PATH] [--port 8080]
```

- [ ] **Step 2: Update key modules table**

Add new modules:

```markdown
| `classify.rs` | Classifier trait, CompositeClassifier |
| `classify/swift.rs` | Built-in Swift framework patterns |
| `classify/rust.rs` | Built-in Rust framework patterns |
| `classify/toml_rules.rs` | User-defined classifier rules |
| `classify/pass.rs` | Post-merge classification pass |
| `config.rs` | grapha.toml configuration parsing |
| `module.rs` | Module map discovery (Swift packages, Cargo workspaces) |
| `query/trace.rs` | Forward dataflow tracing |
| `query/reverse.rs` | Reverse impact to entry points |
| `query/entries.rs` | Entry point listing |
| `serve.rs` | Web UI HTTP server |
```

- [ ] **Step 3: Update architecture section**

Add the extended pipeline description and new edge/node types.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with dataflow features and new commands"
```
