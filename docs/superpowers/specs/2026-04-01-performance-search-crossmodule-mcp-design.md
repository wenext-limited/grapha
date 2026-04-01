# Grapha v2: Performance, Advanced Search, Cross-Module Analysis & MCP

**Date:** 2026-04-01
**Status:** Draft
**Scope:** Six implementation sections + nodus packaging

---

## Motivation

Grapha indexes ~2000-file Swift projects in ~14s, but the extract stage has a slow cold start (plugin init blocks before any file is processed), SQLite writes are unoptimized (~4s for 131K nodes + 764K edges), and search is too basic for large codebases. Comparison with `@wenext/code-context-rag` reveals gaps in agent integration (no MCP server), source snippets for token-efficient browsing, and cross-module analysis for projects with external dependencies.

This design addresses all six areas plus nodus distribution packaging.

---

## Section 1: Pipeline Performance

### 1.1 Lazy/Parallel Plugin Initialization

**Problem:** `prepare_plugins()` and `discover_modules()` run synchronously before extraction begins. For Swift, this scans the Xcode index store and walks for `Package.swift` files.

**Fix:**
- Run `prepare_plugins()` concurrently with `discover_files()` (both are independent).
- Cache the index store path in `.grapha/cache.json` so subsequent runs skip the filesystem scan.
- Show a progress spinner during plugin init so users see activity instead of a blank pause.

### 1.2 Batch SQLite Inserts

**Problem:** Individual `INSERT` per node/edge inside a single transaction (~895K individual executes for lama-ludo).

**Fix:**
- Multi-value batch `INSERT INTO nodes VALUES (...), (...), (...)` with ~500 rows per statement.
- Pre-serialize JSON metadata outside the insert loop (avoid per-row `serde_json::to_string`).
- Sort nodes/edges by ID before insert for B-tree locality.
- Run `PRAGMA optimize` after commit.

**Target:** 4.0s → <1s for the save stage.

### 1.3 Search Index Sync

**Problem:** `sync_index` computes `GraphDelta` independently from the store save, duplicating work.

**Fix:** Reuse the `GraphDelta` already computed during store save, pass it to `sync_index`.

### 1.4 Index Store Opt-In Configuration

Add to `grapha.toml`:

```toml
[swift]
index_store = true  # default true; set false to use tree-sitter only
```

When `false`, the Swift plugin skips index store discovery entirely, falling back to tree-sitter for all files.

---

## Section 2: Advanced Search

### 2.1 Schema Changes

Add indexed fields to the Tantivy schema:

| Field | Type | Purpose |
|-------|------|---------|
| `module` | STRING \| STORED | Filter by module name |
| `visibility` | STRING \| STORED | Filter by pub/crate/private |
| `role` | STRING \| STORED | Filter by entry_point/terminal/internal |

Existing `kind` field (already STRING) becomes queryable via filter logic.

### 2.2 CLI Interface

```bash
grapha search "ViewModel"                              # basic (unchanged)
grapha search "send" --kind function                   # kind filter
grapha search "Config" --kind struct --module FrameUI   # module filter
grapha search "request" --file "Network/**"             # file glob filter
grapha search "view" --role entry_point                 # role filter
grapha search "VeiwModel" --fuzzy                       # fuzzy (Levenshtein distance 2)
grapha search "sendGift" --context                      # inline snippet + deps
grapha search "handle" --kind function --module LamaLudo --context --limit 10
```

### 2.3 Filter Implementation

Filters compose as a Tantivy `BooleanQuery`:
- **Text query** -> BM25 on `name` + `file` fields (existing behavior).
- **`--kind`** -> exact term filter on `kind` field.
- **`--module`** -> exact term filter on `module` field.
- **`--file`** -> glob-to-regex filter on `file` field.
- **`--role`** -> exact term filter on `role` field.
- **`--fuzzy`** -> replace text query with `FuzzyTermQuery` (Levenshtein distance 2).

All filters are AND-composed. Text query is required; filters are optional.

### 2.4 Context Mode

When `--context` is passed, each result includes snippet and direct relationships:

```
sendGift(_:) [function] (LamaLudo)
  src/Gift/GiftService.swift:42
  ---
  func sendGift(_ request: GiftRequest) async throws -> GiftResponse {
      let result = try await network.post("/api/gift/send", body: request)
      ...
  ---
  calls -> FrameNetwork.post(_:body:), GiftTracker.track(_:)
  called_by -> GiftViewModel.onSendTapped(), GiftSheet.sendAction()
  type_refs -> GiftRequest, GiftResponse
```

Without `--context`, output stays compact (name, kind, file, score).

### 2.5 Elapsed Timing

All search/query commands append timing to stderr:

```
12 results in 3.2ms
```

Printed to stderr so JSON output is not polluted when piped.

### 2.6 Web API Fix

Replace the naive `contains()` filter in `serve/api.rs` with the actual `search::search()` call, passing the Tantivy index via `AppState`.

---

## Section 3: Source Snippets

### 3.1 Storage

Add a dedicated field to `Node` in `grapha-core/src/graph.rs`:

```rust
pub snippet: Option<String>,  // <=600 chars of source code
```

A dedicated field (not metadata HashMap) because it's queried frequently, has a clear type/size constraint, and avoids HashMap lookup overhead.

### 3.2 Extraction

Snippet capture happens during the parallel `par_iter` extraction phase while file content is already in memory:

1. Extractor produces `Node` with `span` (start/end line+col).
2. Extract the span range from the already-loaded file content.
3. Truncate to 600 chars at a clean line boundary.
4. Store as `node.snippet = Some(truncated)`.

**Which nodes get snippets:**
- Functions, methods, structs, enums, traits, protocols -> yes
- Properties, fields, variants, constants -> skip (name+type suffices)
- Views, branches (synthetic) -> skip (no real source span)

### 3.3 SQLite Schema

Add `snippet TEXT` column to the `nodes` table (nullable). Included in batch inserts.

### 3.4 Tantivy

Not indexed in Tantivy. Snippets are for display, not search. Retrieved from SQLite when `--context` mode needs them.

### 3.5 Size Budget

For lama-ludo (~131K nodes, ~70% skipped): ~39K nodes x 300 bytes avg = ~12 MB additional storage.

---

## Section 4: Output Customization

### 4.1 Field Selection

A `--fields` flag controls which optional columns appear in output:

| Field | Key | Default | Example |
|-------|-----|---------|---------|
| File path | `file` | on | `src/Gift/GiftService.swift` |
| Symbol ID | `id` | off | `s:7LamaLudo11GiftServiceC...` |
| Module | `module` | off | `LamaLudo` |
| Span | `span` | off | `42:3-58:4` |
| Snippet | `snippet` | off | (source code block) |
| Visibility | `visibility` | off | `public` |
| Signature | `signature` | off | `func sendGift(...)` |
| Role | `role` | off | `entry_point` or `terminal(network)` |

### 4.2 CLI Interface

```bash
grapha context sendGift                          # default: name + kind + file
grapha context sendGift --fields module,signature # add module and signature
grapha context sendGift --fields all             # show everything
grapha context sendGift --fields none            # name + kind only
grapha impact sendGift --fields id,module        # works on all query commands
```

### 4.3 Config Default

```toml
[output]
default_fields = ["file", "module"]  # project-wide defaults
```

CLI `--fields` overrides config (not merges).

### 4.4 Implementation

A `FieldSet` struct parsed from the flag/config:

```rust
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
```

Passed into `render_*_with_options()` via the existing `RenderOptions` struct. JSON output includes fields only when enabled for token efficiency.

### 4.5 Tree Output Example

```
sendGift(_:) [function] public (LamaLudo) src/Gift/GiftService.swift:42
+-- callers (2)
|   +-- GiftViewModel.onSendTapped() [function] (LamaLudo)
|   +-- GiftSheet.sendAction() [function] (LamaLudo)
+-- callees (2)
    +-- FrameNetwork.post(_:body:) [function] (FrameNetwork) [terminal: network]
    +-- GiftTracker.track(_:) [function] (FrameStat) [terminal: event]
```

Fields appear inline, not as sub-items.

---

## Section 5: Cross-Module Graph Analysis

### 5.1 Configuration

External repositories declared in `grapha.toml`:

```toml
[[external]]
name = "FrameUI"
path = "/Users/wendell/developer/WeNext/Frameworks/frameui"

[[external]]
name = "FrameNetwork"
path = "/Users/wendell/developer/WeNext/Frameworks/framenetwork"
```

- `name` is a human label; module names are auto-discovered from `Package.swift`/`Cargo.toml`.
- `path` must be absolute. If missing at index time, warn and skip.

### 5.2 Pipeline Changes

**Discovery:** After main project files, iterate `[[external]]` entries and discover their files too. Tag files with origin (main vs external).

**Module map:** Merge external module maps via existing `ModuleMap::merge()`. The combined map resolves cross-repo edges naturally.

**Merge:** No changes needed. Existing module-aware merge resolves edges by module name from imports.

### 5.3 What This Enables

```bash
grapha impact "ComponentStack" --depth 3
# -> lama-ludo Views that depend on FrameUI.ComponentStack

grapha trace "GiftSheet.body" --depth 10
# -> GiftSheet -> GiftViewModel -> GiftService -> FrameNetwork.post [terminal: network]

grapha search "Stack" --module FrameUI
```

### 5.4 Index Output

```
  ok discovered 1991 files + 460 external (2 repos) (210ms)
  ok extracted 2451 files (8.1s)
```

### 5.5 Storage

External nodes/edges stored in the same SQLite database. The `module` field distinguishes origin. Single unified graph.

### 5.6 File Map Command

```bash
grapha map                    # full project overview
grapha map --module FrameUI   # single module
```

Output:

```
LamaLudo (1991 files, 98432 symbols)
  src/Gift/           12 files   847 symbols
  src/Chat/           28 files  2103 symbols
  src/Profile/         8 files   412 symbols

FrameUI (312 files, 18200 symbols)
  Sources/FrameUI/Components/   45 files  1230 symbols
  Sources/FrameUI/Layout/       18 files   560 symbols
```

Groups by top-level directories within each module.

---

## Section 6: MCP Server Mode

### 6.1 Transport

`grapha serve --mcp` launches JSON-RPC 2.0 over stdio. Can run alongside HTTP: `grapha serve --mcp --port 8080`.

### 6.2 MCP Tools

| Tool | Parameters | Maps To |
|------|-----------|---------|
| `index_project` | `path`, `full_rebuild?` | `grapha index` |
| `search_symbols` | `query`, `kind?`, `module?`, `file?`, `role?`, `fuzzy?`, `context?`, `limit?` | `grapha search` |
| `get_symbol_context` | `symbol`, `path?`, `fields?` | `grapha symbol context` |
| `get_impact` | `symbol`, `depth?`, `path?` | `grapha symbol impact` |
| `get_file_map` | `module?`, `path?` | `grapha map` |
| `trace` | `symbol`, `depth?`, `direction?`, `path?` | `grapha trace` / `grapha reverse` |

### 6.3 Response Format

All tools return structured JSON. Context fields (`snippet`, `calls`, `called_by`) included only when `context: true`.

```json
{
  "results": [
    {
      "name": "sendGift(_:)",
      "kind": "function",
      "file": "src/Gift/GiftService.swift",
      "module": "LamaLudo",
      "score": 1.52,
      "snippet": "func sendGift(...) ...",
      "calls": ["FrameNetwork.post(_:body:)"],
      "called_by": ["GiftViewModel.onSendTapped()"]
    }
  ],
  "elapsed_ms": 3.2,
  "total": 1
}
```

### 6.4 State Management

```rust
struct McpState {
    graph: Graph,
    search_index: tantivy::Index,
    store_path: PathBuf,
}
```

- `index_project` reloads graph and search index after completion.
- All other tools read from in-memory state.
- If no index exists, tools return error: `"project not indexed -- call index_project first"`.

### 6.5 Claude Code Integration

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

### 6.6 Implementation Scope

~300-400 lines: JSON-RPC stdin/stdout reader/writer, tool dispatch to existing handlers, response serialization. No new business logic.

**Out of scope:** MCP Resources, streaming, authentication.

---

## Section 7: Nodus Package

### 7.1 Package Structure

```
nodus/
+-- nodus.toml
+-- skills/
|   +-- grapha.md
+-- rules/
|   +-- grapha-workflow.md
+-- commands/
    +-- index.md
    +-- search.md
    +-- impact.md
```

### 7.2 Skill (`skills/grapha.md`)

Teaches Claude Code grapha-aware exploration:

1. Run `grapha search` to find relevant symbols before reading files.
2. Use `grapha context` to understand relationships.
3. Run `grapha impact` before modifying public APIs.
4. Only read full files for the specific lines needed.

### 7.3 Rules (`rules/grapha-workflow.md`)

Behavior protocols:
- Prefer `grapha search` + `grapha context` over reading entire files.
- Before modifying public APIs, run `grapha impact` to estimate change scope.
- After significant code changes, run `grapha index .` to keep the graph fresh.
- Use `grapha map` to orient in unfamiliar modules.

### 7.4 Commands

- `/index` -> `grapha index .`
- `/search <query>` -> `grapha search "$ARGS" --context` (falls back to `--fuzzy` on no results)
- `/impact <symbol>` -> `grapha impact "$ARGS" --depth 3` with summary

### 7.5 MCP Auto-Configuration

The nodus claude adapter writes `.mcp.json`:

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

### 7.6 Multi-Adapter Support

```bash
nodus add wenext/grapha --adapter claude   # .claude/
nodus add wenext/grapha --adapter cursor   # .cursor/
nodus add wenext/grapha --adapter codex    # .codex/
```

No changes to grapha core code. Purely a packaging layer.

---

## Explicitly Excluded

- **Watch mode** — `grapha index` with incremental is fast enough; can add later.
- **Cross-session recall/notes/hooks** — needs a session tracking layer (separate initiative).
- **Stale memory detection** — depends on recall system.
- **MCP Resources/streaming** — tools-only for now.

---

## Implementation Order

1. **Section 1** (Performance) — foundational, benefits all subsequent work
2. **Section 3** (Source Snippets) — required by Section 2's `--context` mode
3. **Section 2** (Advanced Search) — depends on snippets for context mode
4. **Section 4** (Output Customization) — depends on snippet field existing
5. **Section 5** (Cross-Module) — independent but benefits from perf work
6. **Section 6** (MCP Server) — depends on search filters and context mode
7. **Section 7** (Nodus Package) — depends on MCP server existing
