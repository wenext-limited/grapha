# Flow Command UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `grapha flow entries` usable on large repos and make `grapha flow trace` helpful for SwiftUI-facing symbol queries by adding entry filtering/limiting plus explicit fallback trace roots.

**Architecture:** Keep indexing and graph semantics unchanged. Implement the UX improvement entirely in the CLI/query/render layers by filtering already-classified entry nodes, adding explicit trace fallback root discovery over the existing contains/dataflow graph, and rendering the chosen roots and no-flow hints in output.

**Tech Stack:** Rust, clap, existing `grapha` query/render modules, `assert_cmd` integration tests, existing unit tests in `query` and `render`.

---

### Task 1: Add failing coverage for `flow entries` filtering and limiting

**Files:**
- Modify: `grapha/src/query/entries.rs`
- Modify: `grapha/tests/integration.rs`

- [ ] **Step 1: Write the failing unit tests for filtered entry selection**

```rust
#[test]
fn filters_entries_by_module_and_file_and_limit() {
    let graph = Graph {
        version: "0.1.0".to_string(),
        nodes: vec![
            entry_node("room_body", "body", "Modules/Room/Sources/Room/View/RoomPage.swift", Some("Room")),
            entry_node("room_share", "onShare", "Modules/Room/Sources/Room/View/RoomPage.swift", Some("Room")),
            entry_node("chat_body", "body", "Modules/Chat/Sources/Chat/View/ChatPage.swift", Some("Chat")),
        ],
        edges: vec![],
    };

    let result = query_entries_filtered(
        &graph,
        Some("Room"),
        Some("Modules/Room/Sources/Room/View/RoomPage.swift"),
        Some(1),
    );

    assert_eq!(result.total, 2);
    assert_eq!(result.shown, 1);
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].file, "RoomPage.swift");
}
```

- [ ] **Step 2: Run the targeted unit test to verify it fails**

Run: `cargo test -p grapha filters_entries_by_module_and_file_and_limit -- --nocapture`

Expected: FAIL because `query_entries_filtered` and `shown` do not exist yet.

- [ ] **Step 3: Write the failing integration test for `flow entries --file --limit`**

```rust
#[test]
fn flow_entries_file_scope_and_limit_returns_focused_subset() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("RoomPage.swift"),
        "struct RoomPage {}",
    )
    .unwrap();

    Command::cargo_bin("grapha")
        .unwrap()
        .args([
            "flow",
            "entries",
            "--file",
            "RoomPage.swift",
            "--limit",
            "1",
            "-p",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"shown\": 1"));
}
```

- [ ] **Step 4: Run the targeted integration test to verify it fails**

Run: `cargo test -p grapha flow_entries_file_scope_and_limit_returns_focused_subset -- --nocapture`

Expected: FAIL because the CLI flags and filtered result metadata do not exist yet.

- [ ] **Step 5: Commit the red tests**

```bash
git add grapha/src/query/entries.rs grapha/tests/integration.rs
git commit -m "test(flow): add failing coverage for entry filtering"
```

### Task 2: Implement filtered and limited `flow entries`

**Files:**
- Modify: `grapha/src/main.rs`
- Modify: `grapha/src/query/entries.rs`
- Modify: `grapha/src/query.rs`
- Modify: `grapha/src/render.rs`
- Modify: `grapha/tests/integration.rs`

- [ ] **Step 1: Add CLI flags to `FlowCommands::Entries`**

```rust
Entries {
    #[arg(long)]
    module: Option<String>,
    #[arg(long)]
    file: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
    #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
    format: QueryOutputFormat,
    #[arg(long)]
    fields: Option<String>,
},
```

- [ ] **Step 2: Thread the new flags into the entries query path**

```rust
FlowCommands::Entries {
    module,
    file,
    limit,
    path,
    format,
    fields,
} => {
    let render_options = render_options.with_fields(resolve_field_set(&fields, &path));
    handle_graph_query(
        &path,
        format,
        render_options,
        |graph| query::entries::query_entries_filtered(
            graph,
            module.as_deref(),
            file.as_deref(),
            limit,
        ),
        render::render_entries_with_options,
    )
}
```

- [ ] **Step 3: Implement filtered entry selection and result metadata**

```rust
#[derive(Debug, Serialize)]
pub struct EntriesResult {
    pub entries: Vec<SymbolRef>,
    pub total: usize,
    pub shown: usize,
}

pub fn query_entries_filtered(
    graph: &Graph,
    module: Option<&str>,
    file: Option<&str>,
    limit: Option<usize>,
) -> EntriesResult {
    let mut entries: Vec<SymbolRef> = graph
        .nodes
        .iter()
        .filter(|node| node.role == Some(NodeRole::EntryPoint))
        .filter(|node| module.is_none_or(|expected| node.module.as_deref() == Some(expected)))
        .filter(|node| file.is_none_or(|query| super::file_path_matches_query(&node.file, query)))
        .map(SymbolRef::from_node)
        .collect();

    entries.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });

    let total = entries.len();
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    let shown = entries.len();

    EntriesResult { entries, total, shown }
}
```

- [ ] **Step 4: Update tree rendering to show `shown` vs `total`**

```rust
render_tree(&TreeNode::branch(
    format!(
        "{} ({})",
        Palette::new(options).section_header("entry points"),
        format!("shown={} total={}", result.shown, result.total),
    ),
    children,
))
```

- [ ] **Step 5: Run the targeted tests and confirm they pass**

Run:

```bash
cargo test -p grapha filters_entries_by_module_and_file_and_limit -- --nocapture
cargo test -p grapha flow_entries_file_scope_and_limit_returns_focused_subset -- --nocapture
```

Expected: PASS

- [ ] **Step 6: Commit the entry filtering implementation**

```bash
git add grapha/src/main.rs grapha/src/query/entries.rs grapha/src/query.rs grapha/src/render.rs grapha/tests/integration.rs
git commit -m "feat(flow): add filtered entry listing"
```

### Task 3: Add failing trace fallback tests for SwiftUI roots

**Files:**
- Modify: `grapha/src/query/trace.rs`
- Modify: `grapha/src/render.rs`
- Modify: `grapha/tests/integration.rs`

- [ ] **Step 1: Write the failing unit test for SwiftUI fallback roots**

```rust
#[test]
fn trace_falls_back_to_body_and_action_roots_when_container_has_no_direct_flows() {
    let graph = Graph {
        version: "0.1.0".to_string(),
        nodes: vec![
            node("room-view", NodeKind::Struct, "RoomPageCenterContentView", None),
            node("room-body", NodeKind::Property, "body", None),
            node("room-share", NodeKind::Function, "onShare()", None),
            terminal_node("cache-save", "cacheSave", TerminalKind::Cache),
        ],
        edges: vec![
            contains("room-view", "room-body"),
            contains("room-view", "room-share"),
            writes("room-share", "cache-save", "save"),
        ],
    };

    let result = query_trace(&graph, "RoomPageCenterContentView", 3).unwrap();

    assert!(result.fallback_used);
    assert_eq!(result.traced_roots, vec!["room-share".to_string()]);
    assert_eq!(result.summary.total_flows, 1);
}
```

- [ ] **Step 2: Run the targeted trace unit test to verify it fails**

Run: `cargo test -p grapha trace_falls_back_to_body_and_action_roots_when_container_has_no_direct_flows -- --nocapture`

Expected: FAIL because fallback metadata and fallback traversal do not exist yet.

- [ ] **Step 3: Write the failing unit test for the no-flow hint**

```rust
#[test]
fn trace_returns_hint_when_requested_symbol_and_fallback_roots_have_no_flows() {
    let graph = Graph {
        version: "0.1.0".to_string(),
        nodes: vec![
            node("room-view", NodeKind::Struct, "RoomPageCenterContentView", None),
            node("room-body", NodeKind::Property, "body", None),
        ],
        edges: vec![contains("room-view", "room-body")],
    };

    let result = query_trace(&graph, "RoomPageCenterContentView", 3).unwrap();

    assert_eq!(result.summary.total_flows, 0);
    assert_eq!(
        result.hint.as_deref(),
        Some("no dataflow edges were found from this symbol or its local SwiftUI roots")
    );
}
```

- [ ] **Step 4: Run the targeted no-flow unit test to verify it fails**

Run: `cargo test -p grapha trace_returns_hint_when_requested_symbol_and_fallback_roots_have_no_flows -- --nocapture`

Expected: FAIL because `hint` does not exist yet.

- [ ] **Step 5: Commit the red trace tests**

```bash
git add grapha/src/query/trace.rs grapha/src/render.rs grapha/tests/integration.rs
git commit -m "test(flow): add failing trace fallback coverage"
```

### Task 4: Implement trace fallback roots and explicit metadata

**Files:**
- Modify: `grapha/src/query/trace.rs`
- Modify: `grapha/src/render.rs`
- Modify: `grapha/tests/integration.rs`

- [ ] **Step 1: Extend `TraceResult` with explicit tracing metadata**

```rust
#[derive(Debug, Serialize)]
pub struct TraceResult {
    pub entry: String,
    pub requested_symbol: String,
    pub traced_roots: Vec<String>,
    pub fallback_used: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub flows: Vec<Flow>,
    pub summary: TraceSummary,
    #[serde(skip)]
    pub(crate) entry_ref: SymbolRef,
}
```

- [ ] **Step 2: Extract concrete-root traversal into a helper**

```rust
fn trace_from_root(
    graph: &Graph,
    root_id: &str,
    max_depth: usize,
) -> (Vec<Flow>, TraceSummary) {
    // Move the existing DFS logic here without changing semantics.
}
```

- [ ] **Step 3: Add fallback root discovery over local contains relationships**

```rust
fn fallback_trace_roots(graph: &Graph, requested: &Node) -> Vec<&Node> {
    let parents = crate::localization::edges_by_source(graph);
    let mut roots = Vec::new();

    if matches!(requested.kind, NodeKind::Struct | NodeKind::Class | NodeKind::Property) {
        if let Some(children) = parents.get(requested.id.as_str()) {
            for edge in children {
                if edge.kind != EdgeKind::Contains {
                    continue;
                }
                let Some(child) = graph.nodes.iter().find(|node| node.id == edge.target) else {
                    continue;
                };
                let is_body = child.name == "body";
                let is_action = matches!(
                    child.kind,
                    NodeKind::Function if child.name.starts_with("on")
                        || child.name.starts_with("goto")
                        || child.name.starts_with("handle")
                        || child.name.starts_with("did")
                );
                let has_dataflow_edges = graph.edges.iter().any(|edge| {
                    (edge.source == child.id || edge.target == child.id)
                        && super::flow::is_dataflow_edge(edge.kind)
                });
                if is_body || is_action || has_dataflow_edges {
                    roots.push(child);
                }
            }
        }
    }

    roots.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));
    roots.dedup_by(|left, right| left.id == right.id);
    roots
}
```

- [ ] **Step 4: Assemble the final trace result with fallback metadata and hint**

```rust
let direct = trace_from_root(graph, &entry_node.id, max_depth);
if direct.summary.total_flows > 0 {
    return Ok(TraceResult {
        entry: entry_node.id.clone(),
        requested_symbol: entry_node.id.clone(),
        traced_roots: vec![entry_node.id.clone()],
        fallback_used: false,
        hint: None,
        flows: direct.flows,
        summary: direct.summary,
        entry_ref: SymbolRef::from_node(entry_node),
    });
}

let fallback_roots = fallback_trace_roots(graph, entry_node);
// trace each root, merge flows, set fallback_used and hint accordingly
```

- [ ] **Step 5: Update tree rendering to show requested root, traced roots, and hint**

```rust
let mut children = vec![
    TreeNode::leaf(format_key_value("requested", &result.requested_symbol, options)),
    TreeNode::leaf(format_key_value(
        "traced_roots",
        &result.traced_roots.join(", "),
        options,
    )),
];
if let Some(hint) = result.hint.as_deref() {
    children.push(TreeNode::leaf(format_key_value("hint", hint, options)));
}
```

- [ ] **Step 6: Run the targeted trace tests and confirm they pass**

Run:

```bash
cargo test -p grapha trace_falls_back_to_body_and_action_roots_when_container_has_no_direct_flows -- --nocapture
cargo test -p grapha trace_returns_hint_when_requested_symbol_and_fallback_roots_have_no_flows -- --nocapture
```

Expected: PASS

- [ ] **Step 7: Commit the trace fallback implementation**

```bash
git add grapha/src/query/trace.rs grapha/src/render.rs grapha/tests/integration.rs
git commit -m "feat(flow): add swiftui trace fallback roots"
```

### Task 5: Add real CLI regression coverage and finish verification

**Files:**
- Modify: `grapha/tests/integration.rs`

- [ ] **Step 1: Add the failing integration test for trace fallback metadata**

```rust
#[test]
fn flow_trace_swiftui_symbol_reports_fallback_roots() {
    let temp = TempDir::new().unwrap();
    // build a small indexed fixture with a SwiftUI-like container and action root

    Command::cargo_bin("grapha")
        .unwrap()
        .args(["flow", "trace", "RoomPageCenterContentView", "--format", "json", "-p", temp.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"fallback_used\": true"))
        .stdout(predicates::str::contains("\"traced_roots\""));
}
```

- [ ] **Step 2: Run the targeted integration test and confirm it passes after implementation**

Run: `cargo test -p grapha flow_trace_swiftui_symbol_reports_fallback_roots -- --nocapture`

Expected: PASS

- [ ] **Step 3: Run full verification**

Run:

```bash
cargo fmt -- --check
cargo clippy -p grapha --all-targets -- -D warnings
cargo test -p grapha
cargo build --release -p grapha
```

Expected: all commands pass

- [ ] **Step 4: Validate on the real playground repo**

Run:

```bash
target/release/grapha flow entries --file Modules/Room/Sources/Room/View/RoomPage+Layout.swift --limit 20 -p /Users/wendell/developer/WeNext/lama-ludo-ios --format tree
target/release/grapha flow trace RoomPageCenterContentView --depth 2 -p /Users/wendell/developer/WeNext/lama-ludo-ios --format tree
```

Expected:

- `flow entries` shows a focused subset with `shown` lower than `total`
- `flow trace` either reports fallback roots with non-zero flows or returns an explicit no-flow hint with the traced-root metadata

- [ ] **Step 5: Commit the verified final state**

```bash
git add grapha/tests/integration.rs grapha/src/main.rs grapha/src/query/entries.rs grapha/src/query/trace.rs grapha/src/query.rs grapha/src/render.rs
git commit -m "feat(flow): improve swiftui flow query ergonomics"
```
