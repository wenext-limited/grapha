# Grapha

[中文文档](docs/README.CN.md)

Blazingly fast code intelligence for LLM agents and developer tooling.

Grapha transforms source code into a normalized, graph-based representation with compiler-grade accuracy. For Swift, it reads Xcode's pre-built index store via a binary FFI bridge for fully type-resolved symbol graphs, then falls back to SwiftSyntax and finally tree-sitter for instant parsing without a build. The resulting graph provides persistence, incremental search/index sync, dataflow tracing, semantic effect graphs, and impact analysis — giving agents and developers structured access to codebases at scale.

> **1,991 Swift files — 131K nodes, 787K edges — indexed in 9.7 seconds.**

## Performance

Tested on a production iOS app (1,991 Swift files, ~300K lines):

| Phase | Time |
|-------|------|
| Extraction (index store + tree-sitter enrichment) | **3.6s** |
| Merge (module-aware cross-file resolution) | 0.3s |
| Classification (entry points + terminals) | 1.5s |
| SQLite persistence (deferred index, 918K rows) | 3.1s |
| Search index (BM25 via tantivy, 7 fields) | 0.7s |
| **Total** | **9.7s** |

| Metric | Value |
|--------|-------|
| Nodes (with source snippets) | 131,242 |
| Edges (compiler-resolved) | 787,021 |
| Entry points (auto-detected) | 2,985 |
| Terminal operations | 11,148 |

**Why it's fast:**
- **Zero-parse index-store FFI** — the bridge returns packed structs + a deduplicated string table. Rust reads with pointer arithmetic, no serde on the compiler-grade path.
- **Lock-free parallel extraction** — each rayon thread gets its own extraction context via C callback pointers, no global mutex.
- **Single tree-sitter parse** — one parse shared across doc comment, SwiftUI structure, and localization enrichment passes.
- **Marker-based skip** — files without SwiftUI/localization markers skip expensive enrichment entirely (byte-level scan, not AST).
- **Deferred indexing** — SQLite primary keys and indexes built after bulk insert, not during.
- **USR-scoped edge resolution** — reads edges resolved via USR type prefix matching, eliminating false positives without post-processing.

Use `grapha index --timing` to see a per-phase breakdown including thread-summed extraction times.

## Features

- **Compiler-grade accuracy** — reads Xcode's pre-built index store for 100% type-resolved call graphs (Swift). Falls back to SwiftSyntax, then tree-sitter, for instant parsing without a build.
- **Incremental indexing** — SQLite storage and Tantivy search sync incrementally by default. Use `grapha index --full-rebuild` to force a cold rebuild.
- **Advanced search** — BM25 full-text search with filters (`--kind`, `--module`, `--role`, `--fuzzy`) and context mode (`--context`) that inlines source snippets and relationships.
- **Source snippets** — each symbol stores a truncated source snippet (up to 600 chars) for token-efficient agent browsing without reading full files.
- **Dataflow tracing** — trace forward from entry points to terminal operations (network, persistence, cache), or backward from any symbol to affected entry points.
- **Semantic dataflow graph** — derive a deduplicated effect graph from a symbol with `grapha flow graph`, including reads, writes, publishes, subscribes, and terminal side effects.
- **Impact analysis** — BFS blast radius: "if I change this function, what breaks?"
- **Output customization** — `--fields` flag controls which columns appear (file, id, module, span, snippet, visibility, signature, role). Configurable defaults in `grapha.toml`.
- **Cross-module analysis** — include external local repos via `[[external]]` config for cross-repo edge resolution and impact analysis.
- **File map** — `grapha repo map` shows a directory-level overview of symbol counts per module for project orientation.
- **MCP server** — `grapha serve --mcp` exposes 6 tools over JSON-RPC stdio for AI agent integration (search, context, impact, trace, file map, index).
- **Entry point detection** — auto-detects SwiftUI Views, `@Observable` classes, `fn main()`, `#[test]` functions.
- **Terminal classification** — recognizes network calls, persistence (GRDB, CoreData), cache (Kingfisher), analytics, and more. Extensible via `grapha.toml`.
- **Provenance-aware change detection** — edges carry source spans, so `grapha repo changes` can attribute method-body edits even when declaration spans stay fixed.
- **Web UI** — embedded interactive graph explorer (`grapha serve`).
- **Nodus package** — `nodus add wenext/grapha --adapter claude` for one-liner project setup with skills, rules, and commands.
- **Multi-language** — Rust and Swift today. Architecture supports adding Java, Kotlin, C#, TypeScript.

## Install

```bash
cargo install --path grapha
```

## Quick Start

```bash
# Index a project
grapha index .

# Search with filters
grapha symbol search "ViewModel" --kind struct
grapha symbol search "send" --kind function --module Room --context
grapha symbol search "GiftPanel" --fuzzy

# 360° context for a symbol (callers, callees, reads, implements)
grapha symbol context RoomPage --format tree

# Impact analysis: what breaks if this changes?
grapha symbol impact GiftPanelViewModel --depth 2 --format tree

# Forward trace: entry point → terminal operations (network, persistence, cache)
grapha flow trace RoomPage --format tree

# Reverse: which entry points reach this symbol?
grapha flow trace sendGift --direction reverse --format tree

# Derived semantic effect graph
grapha flow graph GiftPanelViewModel --format tree

# List auto-detected entry points
grapha flow entries

# Project orientation — show modules, directories, symbol counts
grapha repo map --module Room

# Change detection
grapha repo changes

# Interactive web UI
grapha serve --port 8765

# MCP server for AI agents
grapha serve --mcp
```

## Commands

### `grapha index` — Build the graph

```bash
grapha index .                         # Index project (SQLite, incremental)
grapha index . --full-rebuild          # Force full rebuild
grapha index . --timing                # Show per-phase timing breakdown
grapha index . --format json           # JSON output (debugging)
grapha index . --store-dir /tmp/idx    # Custom storage
```

Auto-discovers Xcode's index store from DerivedData for compiler-resolved symbols. Falls back to SwiftSyntax and then tree-sitter when no index is available. SQLite storage and the search index sync incrementally by default.

### `grapha analyze` — One-shot extraction

```bash
grapha analyze src/                    # Analyze directory
grapha analyze src/main.rs             # Single file
grapha analyze src/ --compact          # LLM-optimized grouped output
grapha analyze src/ --filter fn,struct # Filter by symbol kind
grapha analyze src/ -o graph.json      # Write to file
```

### `grapha symbol search` — Full-text search

```bash
grapha symbol search "ViewModel"                            # Basic BM25 search
grapha symbol search "send" --kind function                 # Filter by kind
grapha symbol search "RoomPage" --module Room               # Filter by module
grapha symbol search "view" --role entry_point              # Filter by role
grapha symbol search "GiftPanel" --fuzzy                    # Typo-tolerant
grapha symbol search "Gift" --kind function --context       # Inline snippets + deps
grapha symbol search "handle" --kind function --limit 5     # Combined
```

### `grapha symbol context` — 360° symbol view

```bash
grapha symbol context RoomPage                              # Callers, callees, reads, implements
grapha symbol context RoomPage --format tree                # Tree output
grapha symbol context GiftPanelViewModel --fields module,signature  # Custom fields
```

### `grapha symbol impact` — Blast radius

```bash
grapha symbol impact GiftPanelViewModel                     # Who depends on this?
grapha symbol impact GiftPanelViewModel --depth 3           # Deeper traversal
grapha symbol impact GiftPanelViewModel --format tree
```

### `grapha flow trace` — Forward/reverse dataflow

```bash
grapha flow trace RoomPage                                  # Entry → terminals
grapha flow trace sendGift --depth 10
grapha flow trace sendGift --direction reverse              # Which entries reach this?
grapha flow trace RoomPage --format tree
```

### `grapha flow graph` — Derived semantic effect graph

```bash
grapha flow graph GiftPanelViewModel
grapha flow graph GiftPanelViewModel --depth 10 --format tree
```

### `grapha flow entries` — List entry points

```bash
grapha flow entries
grapha flow entries --format tree
```

### `grapha repo map` — File/symbol overview

```bash
grapha repo map                        # Full project
grapha repo map --module Room          # Single module
```

### `grapha repo changes` — Git change detection

```bash
grapha repo changes                    # All uncommitted changes
grapha repo changes staged             # Only staged
grapha repo changes main               # Compare against branch
```

### `grapha serve` — Web UI and MCP server

```bash
grapha serve --port 8765               # Web UI at http://localhost:8765
grapha serve --mcp                     # MCP server over stdio
```

### `grapha l10n` — Localization

```bash
grapha l10n symbol body                                  # Resolve localization records
grapha l10n usages account_forget_password --table Localizable
```

`--color auto|always|never` controls ANSI styling for tree output. `--fields` controls which columns appear in output (see Output Customization below).

## Configuration

Optional `grapha.toml` at project root:

```toml
[swift]
index_store = true              # Set false to skip index store, use tree-sitter only

[output]
default_fields = ["file", "module"]  # Default fields for all query output

[[external]]
name = "FrameUI"
path = "/path/to/local/frameui"      # Include external repo in the graph

[[external]]
name = "FrameNetwork"
path = "/path/to/local/framenetwork"

[[classifiers]]
pattern = "FirebaseFirestore.*setData"
terminal = "persistence"
direction = "write"
operation = "set"
```

### Output Customization

The `--fields` flag controls which optional columns appear in tree/JSON output:

```bash
grapha symbol context foo --fields module,signature   # Add module and signature
grapha symbol context foo --fields all                # Show everything
grapha symbol context foo --fields none               # Name + kind only
```

Available fields: `file`, `id`, `module`, `span`, `snippet`, `visibility`, `signature`, `role`.

### MCP Server

Add to `.mcp.json` or Claude Code settings:

```json
{
  "mcpServers": {
    "grapha": {
      "command": "grapha",
      "args": ["serve", "--mcp", "--path", "."]
    }
  }
}
```

Tools: `search_symbols`, `get_symbol_context`, `get_impact`, `get_file_map`, `trace`, `index_project`.

### Nodus Package

```bash
nodus add wenext/grapha --adapter claude
```

Installs skills, rules, and slash commands (`/index`, `/search`, `/impact`) for grapha-aware AI workflows.

## Architecture

### Workspace

```
grapha-core/     Shared types (Node, Edge, Graph, ExtractionResult)
grapha-swift/    Swift extraction: index store → SwiftSyntax → tree-sitter waterfall
grapha/          CLI binary, Rust extractor, pipeline, query engines, web UI, MCP server
nodus/           Agent tooling package (skills, rules, commands)
```

### Extraction Waterfall (Swift)

```
1. Xcode Index Store (binary FFI via Swift bridge)
   → compiler-resolved USRs, confidence 1.0
   → auto-discovered from DerivedData
   → concurrent extraction (lock-free per-file context)

2. SwiftSyntax (JSON-string FFI via Swift bridge)
   → accurate parsing, no type resolution, confidence 0.9

3. tree-sitter-swift (bundled)
   → fast fallback, limited accuracy, confidence 0.6-0.8
```

After index store extraction, tree-sitter enriches doc comments, SwiftUI view structure, and localization metadata in a single shared parse. Files without SwiftUI/localization markers skip enrichment entirely.

### Pipeline

```
Discover → Extract → Snippet → Merge → Classify → Compress → Store → Query/Serve
              ↑          ↑        ↑        ↑
      index store /   source   module-   entry points
      SwiftSyntax /   capture  aware     + terminals
      tree-sitter             resolution
```

### Graph Model

Nodes represent symbols (functions, types, properties). Edges represent relationships with confidence scores.

**Node kinds:** `function`, `struct`, `enum`, `trait`, `protocol`, `extension`, `property`, `field`, `variant`, `constant`, `type_alias`, `impl`, `module`, `view`, `branch`

**Edge kinds:**

| Kind | Meaning |
|------|---------|
| `calls` | Function/method call |
| `implements` | Protocol conformance / trait impl |
| `inherits` | Superclass / supertrait |
| `contains` | Structural nesting |
| `type_ref` | Type reference |
| `uses` | Import statement |
| `reads` / `writes` | Data access direction |
| `publishes` / `subscribes` | Event flow |

**Dataflow annotations on edges:**

| Field | Purpose |
|-------|---------|
| `direction` | `read`, `write`, `read_write`, `pure` |
| `operation` | `fetch`, `save`, `publish`, `navigate`, etc. |
| `condition` | Guard/if condition text (when call is conditional) |
| `async_boundary` | Whether call crosses async boundary |
| `provenance` | Source file/span evidence for the relationship |

**Node roles:**
- `entry_point` — SwiftUI View.body, @Observable methods, fn main, #[test]
- `terminal` — network, persistence, cache, event, keychain, search

## Supported Languages

| Language | Extraction | Type Resolution |
|----------|-----------|----------------|
| **Swift** | tree-sitter + Xcode index store | Compiler-grade (USR) |
| **Rust** | tree-sitter | Name-based |

The per-language crate architecture (`grapha-swift/`, future `grapha-java/`, etc.) supports adding new languages with the same pattern: compiler index → syntax parser → tree-sitter fallback.

## Development

```bash
cargo build                    # Build all workspace crates
cargo test                     # Run all tests (~295)
cargo build -p grapha-core     # Build shared types only
cargo build -p grapha-swift    # Build Swift extractor
cargo run -p grapha -- <cmd>   # Run the CLI
cargo clippy                   # Lint
cargo fmt                      # Format
```

## License

MIT
