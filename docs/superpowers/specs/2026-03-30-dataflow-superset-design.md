# Dataflow Analysis Superset — Design Spec

**Date:** 2026-03-30
**Goal:** Make grapha a superset of `wenext-api-dataflow-explore` capabilities, targeting Rust and Swift codebases (not Java). Demonstrate Rust's speed advantage for internal company adoption.
**Demo targets:** `lama-ludo-ios` (complex SwiftUI iOS project with multiple Swift packages) and grapha itself (Rust dogfooding).

---

## 1. Enriched Graph Model

Extend `Node` and `Edge` with optional fields for dataflow analysis. All new fields are `Option` to preserve backward compatibility with existing serialization and tests.

### Node Extensions

```rust
/// What role this node plays in dataflow analysis
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    EntryPoint,
    Terminal { kind: TerminalKind },
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    Network,       // URLSession, Alamofire, Moya, reqwest
    Persistence,   // CoreData, Realm, UserDefaults, rusqlite, std::fs
    Cache,         // NSCache, in-memory caches
    Event,         // NotificationCenter, Combine, tokio channels
    Keychain,      // Keychain access
    Search,        // Tantivy, Spotlight
}

pub struct Node {
    // existing fields unchanged
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub file: PathBuf,
    pub span: Span,
    pub visibility: Visibility,
    pub metadata: HashMap<String, String>,
    // new fields
    pub role: Option<NodeRole>,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub module: Option<String>,
}
```

### Edge Extensions

```rust
pub enum EdgeKind {
    // existing
    Calls, Uses, Implements, Contains, TypeRef, Inherits,
    // new dataflow-specific
    Reads,
    Writes,
    Publishes,
    Subscribes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    Read,
    Write,
    ReadWrite,
    Pure,
}

pub struct Edge {
    // existing fields unchanged
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub confidence: f64,
    // new fields
    pub direction: Option<FlowDirection>,
    pub operation: Option<String>,
    pub condition: Option<String>,
    pub async_boundary: Option<bool>,
}
```

---

## 2. Classifier System

Trait-based classifiers that recognize framework-specific patterns and enrich call edges with dataflow metadata.

### Classifier Trait

```rust
pub trait Classifier {
    fn classify(&self, call_target: &str, context: &ClassifyContext) -> Option<Classification>;
}

pub struct ClassifyContext {
    pub source_node: String,
    pub file: PathBuf,
    pub arguments: Vec<String>,
}

pub struct Classification {
    pub terminal_kind: TerminalKind,
    pub direction: FlowDirection,
    pub operation: String,
}
```

### Resolution Order

`CompositeClassifier` chains classifiers, first match wins:

1. **User-defined** (`grapha.toml` `[[classifiers]]` rules)
2. **Built-in** (`SwiftClassifier` / `RustClassifier`)

### Built-in Swift Patterns

| Pattern | Terminal | Direction | Operation |
|---------|---------|-----------|-----------|
| `URLSession.*dataTask`, `AF.request`, `Moya` | Network | Read | fetch |
| `URLSession.*upload`, `AF.upload` | Network | Write | upload |
| `NSManagedObjectContext.save`, `realm.write` | Persistence | Write | save |
| `NSManagedObjectContext.fetch`, `realm.objects` | Persistence | Read | fetch |
| `UserDefaults.set` | Persistence | Write | set |
| `UserDefaults.*forKey` | Persistence | Read | get |
| `KeychainWrapper.set`, `SecItemAdd` | Keychain | Write | set |
| `KeychainWrapper.*forKey` | Keychain | Read | get |
| `NotificationCenter.post` | Event | Write | publish |
| `NotificationCenter.addObserver` | Event | Read | subscribe |
| `PassthroughSubject.send` | Event | Write | publish |
| `NSCache.setObject` | Cache | Write | set |
| `NSCache.object` | Cache | Read | get |

### Built-in Rust Patterns

| Pattern | Terminal | Direction | Operation |
|---------|---------|-----------|-----------|
| `std::fs::read*`, `File::open` (read) | Persistence | Read | read |
| `std::fs::write*`, `File::create` | Persistence | Write | write |
| `rusqlite::Connection.execute` | Persistence | Write | execute |
| `rusqlite::Connection.query*` | Persistence | Read | query |
| `tantivy::IndexWriter.*` | Search | Write | index |
| `tantivy::Searcher.*` | Search | Read | search |
| `tokio::sync::mpsc::Sender.send` | Event | Write | send |
| `tokio::sync::mpsc::Receiver.recv` | Event | Read | receive |
| `reqwest::Client.*` | Network | Read | fetch |

### User-Defined Classifiers (`grapha.toml`)

```toml
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
```

User rules take priority over built-in patterns and can override them.

**Note:** Classifiers run as a post-extraction pass (pipeline step 3), not during extraction. This ensures cross-file edges from the merge step are also classified.

---

## 3. Dataflow Tracing Engine

Forward tracing from entry points to terminal operations, with condition and async boundary tracking.

### Algorithm

```
trace(entry_node) → TraceResult:
  1. Find entry node (auto-detected or user-specified)
  2. BFS/DFS forward along Calls edges
  3. At each call site:
     a. Run classifier — if terminal, record flow edge
     b. Walk up CST for enclosing if/guard/switch — record condition
     c. Detect async boundary (await, spawn, Task {}) — mark edge
     d. Continue recursion into callee
  4. Propagate write direction upward through chain
  5. Return: entry → intermediate calls → terminal ops (with conditions)
```

### Condition Extraction

During tree-sitter extraction, walk up from call node to find enclosing conditional:

- **Swift:** `if_statement`, `guard_statement`, `switch_case`
- **Rust:** `if_expression`, `match_arm`, `if_let_expression`

Extract the condition text and attach to the edge.

### Async Boundary Detection

- **Swift:** `await`, `Task { }`, `DispatchQueue.async`, `.task { }` modifier
- **Rust:** `.await`, `tokio::spawn`, `std::thread::spawn`

### TraceResult Output

```json
{
  "entry": "ContentView::body",
  "flows": [
    {
      "path": ["ContentView::body", "UserViewModel::loadUser", "APIClient::fetchUser"],
      "terminal": { "kind": "network", "operation": "fetch", "direction": "read" },
      "conditions": ["viewModel.userId != nil"],
      "async_boundaries": ["UserViewModel::loadUser → APIClient::fetchUser"]
    }
  ],
  "summary": {
    "total_flows": 2,
    "reads": 1,
    "writes": 1,
    "async_crossings": 1
  }
}
```

### New CLI Subcommands

```
grapha trace <entry> [-p PATH] [--depth N]   — forward dataflow from entry to terminals
grapha reverse <symbol> [-p PATH]            — which entry points are affected by this symbol?
grapha entries [-p PATH]                      — list auto-detected entry points
```

---

## 4. Entry Point Auto-Detection

Happens during extraction — zero extra cost.

### Swift (SwiftUI) Rules

| Pattern | Detection Method |
|---------|-----------------|
| `View` conformance | struct conforming to `View` → `body` property is entry |
| `App` conformance | `@main` + conforms to `App` |
| `@Observable` / `ObservableObject` | public methods on these classes are entries |
| Button/gesture actions | closures in `Button(action:)`, `onTapGesture`, `onAppear`, `.task` |
| `init()` on Views | initializers of View-conforming types |
| `PreviewProvider` | `previews` property |

### Rust Rules

| Pattern | Detection Method |
|---------|-----------------|
| `fn main()` | top-level main function |
| `#[test]` | test-attributed functions |
| `#[tokio::main]` | async main entry |
| `pub fn` at crate root | public API surface |
| Web framework attributes | `#[get]`, `#[post]`, `#[route]` (extensible via `grapha.toml`) |

### User Override (`grapha.toml`)

```toml
[[entry_points]]
language = "swift"
pattern = ".*Coordinator.start"

[[entry_points]]
language = "rust"
attribute = "actix_web::get"
```

---

## 5. Cross-Module / Cross-Package Support

### Swift Package Discovery

1. Scan for `Package.swift` files — each defines a package with targets
2. Build module map: package name → source directories → symbols
3. Module-aware resolution during merge

### Rust Workspace Support

1. Scan for `Cargo.toml` with `[workspace]` — discover member crates
2. Build crate map: crate name → `src/` → symbols
3. `use other_crate::Foo` resolves to actual symbol

### Resolution Confidence (Enhanced)

| Scope | Confidence |
|-------|-----------|
| Same file | 1.0 |
| Same package/module | 0.9 |
| Cross-package, unambiguous | 0.8 |
| Cross-package, ambiguous | 0.5 |

### Module Metadata

Nodes gain `module: Option<String>` — enables module-level filtering, grouping in web UI, and cross-module impact reporting.

### Boundaries

- No automatic `git submodule update` — analyze whatever source is on disk
- No Xcode build system integration — parse source files only
- No inter-process / network-level tracing

---

## 6. Web UI — `grapha serve`

Embedded web server for interactive graph exploration. "Have a glance" — visually impressive for demos, not a full IDE.

### Architecture

```
grapha serve [-p PATH] [--port 8080]
    │
    ├── Embedded HTTP server (axum)
    │   ├── GET /                → Single-page app (embedded static assets)
    │   ├── GET /api/graph       → Full graph JSON
    │   ├── GET /api/trace/:sym  → Forward trace result
    │   ├── GET /api/reverse/:sym → Reverse impact to entries
    │   ├── GET /api/context/:sym → 360° symbol context
    │   ├── GET /api/search?q=   → BM25 search
    │   └── GET /api/entries     → List auto-detected entry points
    │
    └── Static assets compiled into binary via include_str!
```

### Frontend

**vis-network** (same library dataflow-explore uses):

- Single JS file, no build step, no npm
- DAG layout with zoom, pan, click-to-select
- Nodes color-coded by role: entry=green, terminal=red/orange/blue by kind, internal=gray
- Edges styled by direction: write=red, read=blue, pure=gray, dashed=async

### UI Layout

```
┌─────────────────────────────────────────────────────┐
│  [Search: ________]  [Entries ▼]  [Filter ▼]        │
├──────────────────────────────┬──────────────────────┤
│                              │  Symbol Detail        │
│                              │  Name: fetchUser      │
│      Graph Canvas            │  Kind: function        │
│      (vis-network)           │  Role: terminal(net)   │
│                              │  Direction: read       │
│                              │  Signature: ...        │
│                              │  Conditions: ...       │
│                              │  Callers / Callees     │
└──────────────────────────────┴──────────────────────┘
```

### Interaction

- Click node → detail panel + highlight connected edges
- Click entry point → auto-run trace, highlight full flow path
- Search → filter/highlight matching nodes
- Filter → show/hide by node kind, role, or terminal type

---

## 7. Pipeline Integration

### Extended Pipeline

```
1. Discover   — files + Package.swift / Cargo.toml for module map
2. Extract    — tree-sitter walk + entry point detection + condition extraction + signatures
3. Classify   — run CompositeClassifier on Calls edges → enrich with direction/operation
4. Merge      — module-aware cross-file resolution
5. Compress   — existing prune + group (new fields pass through)
6. Store      — SQLite schema gains new nullable columns
7. Query      — existing commands unchanged + trace + reverse + entries
8. Serve      — embedded web UI
```

### Updated CLI

```
grapha analyze <path> [--output FILE] [--filter KINDS] [--compact]   # unchanged
grapha index <path> [--format sqlite|json] [--store-dir DIR]         # unchanged
grapha context <symbol> [-p PATH]                                     # unchanged
grapha impact <symbol> [--depth N] [-p PATH]                          # unchanged
grapha search <query> [--limit N] [-p PATH]                           # unchanged
grapha changes [SCOPE] [-p PATH]                                      # unchanged

grapha trace <entry> [-p PATH] [--depth N]     # NEW
grapha reverse <symbol> [-p PATH]              # NEW
grapha entries [-p PATH]                        # NEW
grapha serve [-p PATH] [--port 8080]           # NEW
```

### New Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP server for `serve` |
| `tokio` (minimal features) | async runtime for axum |
| `toml` | parse `grapha.toml` config |

### Migration Strategy

- All new fields are `Option` → existing data still loads
- SQLite: `ALTER TABLE ADD COLUMN` for each new nullable column
- Existing 79 tests remain untouched
- `grapha.toml` is optional — zero-config by default

### Out of Scope

- No Java/Kotlin/TypeScript language support
- No Neo4j backend
- No runtime profiling / dynamic analysis
- No Xcode build system integration
- No inter-process / network-level tracing
