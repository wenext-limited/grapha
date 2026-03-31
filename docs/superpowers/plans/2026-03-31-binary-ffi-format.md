# Binary FFI Format Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace JSON serialization/deserialization in the Swift-Rust IndexStore FFI bridge with a flat binary buffer format for zero-parse extraction.

**Architecture:** Swift builds a single `malloc`'d buffer (header + packed node/edge arrays + deduplicated string table). Rust reads it with offset arithmetic — no serde, no intermediate allocations. The FFI signature changes from returning a JSON C string to returning a binary buffer pointer + length out-parameter.

**Tech Stack:** Swift 6.3 (`UnsafeMutableRawPointer`, `withMemoryRebound`), Rust (`std::slice::from_raw_parts`, little-endian `u32`/`u8` reads)

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `grapha-swift/src/binary.rs` | **Create** | Rust binary buffer parser (`parse_binary_buffer`) |
| `grapha-swift/src/indexstore.rs` | **Modify** | Call binary parser instead of `serde_json` |
| `grapha-swift/src/bridge.rs` | **Modify** | Update FFI function pointer types |
| `grapha-swift/src/lib.rs` | **Modify** | Add `mod binary;` |
| `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/Bridge.swift` | **Modify** | Update FFI export signature, add `free_buffer` |
| `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/IndexStoreReader.swift` | **Modify** | Replace `buildJSON`/`appendEscaped` with `buildBinaryBuffer` |

---

### Task 1: Rust Binary Buffer Parser

**Files:**
- Create: `grapha-swift/src/binary.rs`
- Modify: `grapha-swift/src/lib.rs` (add `mod binary;`)

This is the Rust-side parser that reads the binary buffer and produces an `ExtractionResult`. It is fully testable with synthetic buffers — no Swift bridge needed.

- [ ] **Step 1: Write failing test for header validation**

In `grapha-swift/src/binary.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use grapha_core::graph::{Edge, EdgeKind, Node, NodeKind, Span, Visibility};
use grapha_core::ExtractionResult;

const MAGIC: u32 = 0x47524148; // "GRAH"
const VERSION: u8 = 1;
const HEADER_SIZE: usize = 20;
const PACKED_NODE_SIZE: usize = 44;
const PACKED_EDGE_SIZE: usize = 20;
const NO_MODULE: u32 = 0xFFFFFFFF;

pub fn parse_binary_buffer(buf: &[u8]) -> Option<ExtractionResult> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a minimal valid buffer with 0 nodes, 0 edges, empty string table.
    fn empty_buffer() -> Vec<u8> {
        let mut buf = Vec::with_capacity(HEADER_SIZE);
        buf.extend_from_slice(&MAGIC.to_le_bytes());       // magic
        buf.push(VERSION);                                   // version
        buf.extend_from_slice(&[0u8; 3]);                    // pad
        buf.extend_from_slice(&0u32.to_le_bytes());          // node_count
        buf.extend_from_slice(&0u32.to_le_bytes());          // edge_count
        buf.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes()); // string_table_offset
        buf
    }

    #[test]
    fn test_empty_buffer_parses() {
        let buf = empty_buffer();
        let result = parse_binary_buffer(&buf).unwrap();
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
        assert!(result.imports.is_empty());
    }

    #[test]
    fn test_bad_magic_returns_none() {
        let mut buf = empty_buffer();
        buf[0] = 0xFF; // corrupt magic
        assert!(parse_binary_buffer(&buf).is_none());
    }

    #[test]
    fn test_bad_version_returns_none() {
        let mut buf = empty_buffer();
        buf[4] = 99; // unknown version
        assert!(parse_binary_buffer(&buf).is_none());
    }

    #[test]
    fn test_truncated_buffer_returns_none() {
        let buf = vec![0u8; 10]; // too short for header
        assert!(parse_binary_buffer(&buf).is_none());
    }
}
```

- [ ] **Step 2: Register the module**

In `grapha-swift/src/lib.rs`, add after `mod bridge;`:

```rust
mod binary;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p grapha-swift -- binary`
Expected: 4 failures (all hit `todo!()`)

- [ ] **Step 4: Implement header parsing**

Replace `todo!()` in `parse_binary_buffer`:

```rust
pub fn parse_binary_buffer(buf: &[u8]) -> Option<ExtractionResult> {
    if buf.len() < HEADER_SIZE {
        return None;
    }

    let magic = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }

    let version = buf[4];
    if version != VERSION {
        return None;
    }

    let node_count = u32::from_le_bytes(buf[8..12].try_into().ok()?) as usize;
    let edge_count = u32::from_le_bytes(buf[12..16].try_into().ok()?) as usize;
    let string_table_offset = u32::from_le_bytes(buf[16..20].try_into().ok()?) as usize;

    let expected_data_end = HEADER_SIZE + node_count * PACKED_NODE_SIZE + edge_count * PACKED_EDGE_SIZE;
    if string_table_offset != expected_data_end || string_table_offset > buf.len() {
        return None;
    }

    let string_table = &buf[string_table_offset..];

    let mut nodes = Vec::with_capacity(node_count);
    let mut edges = Vec::with_capacity(edge_count);

    for i in 0..node_count {
        let base = HEADER_SIZE + i * PACKED_NODE_SIZE;
        nodes.push(read_node(&buf[base..base + PACKED_NODE_SIZE], string_table)?);
    }

    let edges_base = HEADER_SIZE + node_count * PACKED_NODE_SIZE;
    for i in 0..edge_count {
        let base = edges_base + i * PACKED_EDGE_SIZE;
        edges.push(read_edge(&buf[base..base + PACKED_EDGE_SIZE], string_table)?);
    }

    Some(ExtractionResult { nodes, edges, imports: vec![] })
}
```

- [ ] **Step 5: Run tests to verify header tests pass**

Run: `cargo test -p grapha-swift -- binary`
Expected: header tests pass, but `read_node`/`read_edge` don't exist yet — add stubs or implement in next step.

- [ ] **Step 6: Implement `read_node` and `read_edge` helpers**

Add to `grapha-swift/src/binary.rs`:

```rust
fn read_str<'a>(string_table: &'a [u8], offset: u32, len: u32) -> Option<&'a str> {
    let start = offset as usize;
    let end = start + len as usize;
    if end > string_table.len() {
        return None;
    }
    std::str::from_utf8(&string_table[start..end]).ok()
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
}

fn decode_node_kind(v: u8) -> Option<NodeKind> {
    match v {
        0 => Some(NodeKind::Function),
        1 => Some(NodeKind::Struct),
        2 => Some(NodeKind::Enum),
        3 => Some(NodeKind::Protocol),
        4 => Some(NodeKind::Extension),
        5 => Some(NodeKind::TypeAlias),
        6 => Some(NodeKind::Property),
        7 => Some(NodeKind::Field),
        8 => Some(NodeKind::Variant),
        _ => None,
    }
}

fn decode_edge_kind(v: u8) -> Option<EdgeKind> {
    match v {
        0 => Some(EdgeKind::Calls),
        1 => Some(EdgeKind::Contains),
        2 => Some(EdgeKind::Inherits),
        3 => Some(EdgeKind::Implements),
        4 => Some(EdgeKind::TypeRef),
        _ => None,
    }
}

fn decode_visibility(v: u8) -> Option<Visibility> {
    match v {
        0 => Some(Visibility::Public),
        1 => Some(Visibility::Crate),
        2 => Some(Visibility::Private),
        _ => None,
    }
}

fn read_node(chunk: &[u8], string_table: &[u8]) -> Option<Node> {
    let id = read_str(string_table, read_u32(chunk, 0), read_u32(chunk, 4))?.to_owned();
    let name = read_str(string_table, read_u32(chunk, 8), read_u32(chunk, 12))?.to_owned();
    let file = read_str(string_table, read_u32(chunk, 16), read_u32(chunk, 20))?.to_owned();
    let module_offset = read_u32(chunk, 24);
    let module = if module_offset == NO_MODULE {
        None
    } else {
        Some(read_str(string_table, module_offset, read_u32(chunk, 28))?.to_owned())
    };
    let line = read_u32(chunk, 32) as usize;
    let col = read_u32(chunk, 36) as usize;
    let kind = decode_node_kind(chunk[40])?;
    let visibility = decode_visibility(chunk[41])?;

    Some(Node {
        id,
        kind,
        name,
        file: PathBuf::from(file),
        span: Span { start: [line, col], end: [line, col] },
        visibility,
        metadata: HashMap::new(),
        role: None,
        signature: None,
        doc_comment: None,
        module,
    })
}

fn read_edge(chunk: &[u8], string_table: &[u8]) -> Option<Edge> {
    let source = read_str(string_table, read_u32(chunk, 0), read_u32(chunk, 4))?.to_owned();
    let target = read_str(string_table, read_u32(chunk, 8), read_u32(chunk, 12))?.to_owned();
    let kind = decode_edge_kind(chunk[16])?;
    let confidence = chunk[17] as f64 / 100.0;

    Some(Edge {
        source,
        target,
        kind,
        confidence,
        direction: None,
        operation: None,
        condition: None,
        async_boundary: None,
    })
}
```

- [ ] **Step 7: Add node/edge round-trip test**

Add to the `tests` module in `binary.rs`:

```rust
/// Helper: append a string to the string table, return (offset, len).
fn append_str(string_table: &mut Vec<u8>, s: &str) -> (u32, u32) {
    let offset = string_table.len() as u32;
    string_table.extend_from_slice(s.as_bytes());
    (offset, s.len() as u32)
}

/// Helper: write a PackedNode into a buffer.
fn write_node(
    buf: &mut Vec<u8>,
    id: (u32, u32), name: (u32, u32), file: (u32, u32),
    module: Option<(u32, u32)>, line: u32, col: u32, kind: u8, vis: u8,
) {
    buf.extend_from_slice(&id.0.to_le_bytes());
    buf.extend_from_slice(&id.1.to_le_bytes());
    buf.extend_from_slice(&name.0.to_le_bytes());
    buf.extend_from_slice(&name.1.to_le_bytes());
    buf.extend_from_slice(&file.0.to_le_bytes());
    buf.extend_from_slice(&file.1.to_le_bytes());
    match module {
        Some(m) => {
            buf.extend_from_slice(&m.0.to_le_bytes());
            buf.extend_from_slice(&m.1.to_le_bytes());
        }
        None => {
            buf.extend_from_slice(&NO_MODULE.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
    }
    buf.extend_from_slice(&line.to_le_bytes());
    buf.extend_from_slice(&col.to_le_bytes());
    buf.push(kind);
    buf.push(vis);
    buf.extend_from_slice(&[0u8; 2]); // pad
}

/// Helper: write a PackedEdge into a buffer.
fn write_edge(
    buf: &mut Vec<u8>,
    source: (u32, u32), target: (u32, u32), kind: u8, confidence_pct: u8,
) {
    buf.extend_from_slice(&source.0.to_le_bytes());
    buf.extend_from_slice(&source.1.to_le_bytes());
    buf.extend_from_slice(&target.0.to_le_bytes());
    buf.extend_from_slice(&target.1.to_le_bytes());
    buf.push(kind);
    buf.push(confidence_pct);
    buf.extend_from_slice(&[0u8; 2]); // pad
}

#[test]
fn test_one_node_one_edge() {
    let mut string_table = Vec::new();
    let id = append_str(&mut string_table, "s:MyModule.MyStruct");
    let name = append_str(&mut string_table, "MyStruct");
    let file = append_str(&mut string_table, "MyStruct.swift");
    let module = append_str(&mut string_table, "MyModule");
    let target_id = append_str(&mut string_table, "s:MyModule.MyProtocol");

    let node_count: u32 = 1;
    let edge_count: u32 = 1;
    let str_table_off = (HEADER_SIZE + PACKED_NODE_SIZE + PACKED_EDGE_SIZE) as u32;

    // Build buffer
    let mut buf = Vec::new();
    // Header
    buf.extend_from_slice(&MAGIC.to_le_bytes());
    buf.push(VERSION);
    buf.extend_from_slice(&[0u8; 3]);
    buf.extend_from_slice(&node_count.to_le_bytes());
    buf.extend_from_slice(&edge_count.to_le_bytes());
    buf.extend_from_slice(&str_table_off.to_le_bytes());
    // Node
    write_node(&mut buf, id, name, file, Some(module), 10, 5, 1, 0); // struct, public
    // Edge
    write_edge(&mut buf, id, target_id, 3, 100); // implements, 1.0
    // String table
    buf.extend_from_slice(&string_table);

    let result = parse_binary_buffer(&buf).unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.edges.len(), 1);

    let node = &result.nodes[0];
    assert_eq!(node.id, "s:MyModule.MyStruct");
    assert_eq!(node.name, "MyStruct");
    assert_eq!(node.kind, NodeKind::Struct);
    assert_eq!(node.visibility, Visibility::Public);
    assert_eq!(node.module.as_deref(), Some("MyModule"));
    assert_eq!(node.span.start, [10, 5]);

    let edge = &result.edges[0];
    assert_eq!(edge.source, "s:MyModule.MyStruct");
    assert_eq!(edge.target, "s:MyModule.MyProtocol");
    assert_eq!(edge.kind, EdgeKind::Implements);
    assert!((edge.confidence - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_string_out_of_bounds_returns_none() {
    let mut buf = empty_buffer();
    // Change to 1 node, 0 edges
    buf[8..12].copy_from_slice(&1u32.to_le_bytes());
    // Update string_table_offset
    let st_off = (HEADER_SIZE + PACKED_NODE_SIZE) as u32;
    buf[16..20].copy_from_slice(&st_off.to_le_bytes());
    // Write a node with out-of-bounds string offset
    let mut node = vec![0u8; PACKED_NODE_SIZE];
    node[0..4].copy_from_slice(&9999u32.to_le_bytes()); // id_offset: way out of bounds
    node[4..8].copy_from_slice(&5u32.to_le_bytes());    // id_len
    node[40] = 0; // kind: function
    node[41] = 0; // visibility: public
    buf.extend_from_slice(&node);
    // Empty string table
    assert!(parse_binary_buffer(&buf).is_none());
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test -p grapha-swift -- binary`
Expected: all 6 tests pass

- [ ] **Step 9: Commit**

```bash
git add grapha-swift/src/binary.rs grapha-swift/src/lib.rs
git commit -m "feat(swift): add binary buffer parser for FFI extraction"
```

---

### Task 2: Update Rust FFI Bridge Types

**Files:**
- Modify: `grapha-swift/src/bridge.rs`

- [ ] **Step 1: Update the function pointer types and struct**

In `grapha-swift/src/bridge.rs`, change:

```rust
type IndexStoreExtractFn = unsafe extern "C" fn(*mut std::ffi::c_void, *const i8) -> *const i8;
```

to:

```rust
type IndexStoreExtractFn = unsafe extern "C" fn(*mut std::ffi::c_void, *const i8, *mut u32) -> *const u8;
```

Add after `FreeStringFn`:

```rust
type FreeBufferFn = unsafe extern "C" fn(*mut u8);
```

Add `free_buffer` field to `SwiftBridge`:

```rust
pub struct SwiftBridge {
    _lib: Library,
    pub indexstore_open: IndexStoreOpenFn,
    pub indexstore_extract: IndexStoreExtractFn,
    pub indexstore_close: IndexStoreCloseFn,
    pub swiftsyntax_extract: SwiftSyntaxExtractFn,
    pub free_string: FreeStringFn,
    pub free_buffer: FreeBufferFn,
}
```

In `SwiftBridge::load()`, add before the `Some(SwiftBridge { ... })`:

```rust
let free_buffer = *lib.get::<FreeBufferFn>(b"grapha_free_buffer").ok()?;
```

And add `free_buffer,` to the struct literal.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p grapha-swift`
Expected: compiles (the dylib isn't loaded at compile time)

- [ ] **Step 3: Commit**

```bash
git add grapha-swift/src/bridge.rs
git commit -m "feat(swift): update FFI types for binary buffer extraction"
```

---

### Task 3: Wire Binary Parser into IndexStore Extraction

**Files:**
- Modify: `grapha-swift/src/indexstore.rs`

- [ ] **Step 1: Replace JSON parsing with binary buffer parsing**

Rewrite `extract_from_indexstore` in `grapha-swift/src/indexstore.rs`:

```rust
use std::ffi::CString;
use std::path::Path;
use std::sync::OnceLock;

use grapha_core::ExtractionResult;

use crate::binary;
use crate::bridge;

/// Cached store handle — opened once, reused for all files.
static STORE_HANDLE: OnceLock<Option<StoreHandle>> = OnceLock::new();

struct StoreHandle {
    ptr: *mut std::ffi::c_void,
}

// Store handles are thread-safe (protected by lock on Swift side)
unsafe impl Send for StoreHandle {}
unsafe impl Sync for StoreHandle {}

fn get_or_open_store(index_store_path: &Path) -> Option<*mut std::ffi::c_void> {
    let handle = STORE_HANDLE.get_or_init(|| {
        let bridge = bridge::bridge()?;
        let path_c = CString::new(index_store_path.to_str()?).ok()?;
        let ptr = unsafe { (bridge.indexstore_open)(path_c.as_ptr()) };
        if ptr.is_null() {
            None
        } else {
            Some(StoreHandle { ptr })
        }
    });
    handle.as_ref().map(|h| h.ptr)
}

/// Try to extract Swift symbols from Xcode's index store.
pub fn extract_from_indexstore(
    file_path: &Path,
    index_store_path: &Path,
) -> Option<ExtractionResult> {
    let bridge = bridge::bridge()?;
    let handle = get_or_open_store(index_store_path)?;

    let file_path_c = CString::new(file_path.to_str()?).ok()?;
    let mut buf_len: u32 = 0;
    let buf_ptr = unsafe {
        (bridge.indexstore_extract)(handle, file_path_c.as_ptr(), &mut buf_len)
    };

    if buf_ptr.is_null() || buf_len == 0 {
        return None;
    }

    let buf = unsafe { std::slice::from_raw_parts(buf_ptr, buf_len as usize) };
    let result = binary::parse_binary_buffer(buf);
    unsafe { (bridge.free_buffer)(buf_ptr as *mut u8) };

    result
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p grapha-swift`
Expected: compiles (serde_json import is now unused — remove it from Cargo.toml if desired, or leave for tree-sitter)

- [ ] **Step 3: Commit**

```bash
git add grapha-swift/src/indexstore.rs
git commit -m "feat(swift): wire binary parser into indexstore extraction"
```

---

### Task 4: Swift Bridge FFI Exports

**Files:**
- Modify: `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/Bridge.swift`

- [ ] **Step 1: Update `indexstoreExtract` signature and add `freeBuffer`**

Replace the full contents of `Bridge.swift`:

```swift
import Foundation
import Synchronization

// MARK: - Reader Storage

private let _readers = Mutex<[Int: IndexStoreReader]>([:])
private let _nextHandle = Atomic<Int>(1)

// MARK: - Index Store

@c(grapha_indexstore_open)
public func indexstoreOpen(_ path: UnsafePointer<CChar>) -> UnsafeMutableRawPointer? {
    let pathStr = String(cString: path)
    guard let reader = IndexStoreReader(storePath: pathStr) else { return nil }
    let handle = _nextHandle.wrappingAdd(1, ordering: .relaxed).oldValue
    _readers.withLock { $0[handle] = reader }
    return UnsafeMutableRawPointer(bitPattern: handle)
}

@c(grapha_indexstore_extract)
public func indexstoreExtract(
    _ handle: UnsafeMutableRawPointer,
    _ filePath: UnsafePointer<CChar>,
    _ outLen: UnsafeMutablePointer<UInt32>
) -> UnsafeRawPointer? {
    let key = Int(bitPattern: handle)
    let reader = _readers.withLock { $0[key] }
    guard let reader else { return nil }
    let file = String(cString: filePath)
    guard let (ptr, len) = reader.extractFile(file) else { return nil }
    outLen.pointee = len
    return UnsafeRawPointer(ptr)
}

@c(grapha_indexstore_close)
public func indexstoreClose(_ handle: UnsafeMutableRawPointer) {
    let key = Int(bitPattern: handle)
    _ = _readers.withLock { $0.removeValue(forKey: key) }
}

// MARK: - SwiftSyntax

@c(grapha_swiftsyntax_extract)
public func swiftsyntaxExtract(
    _ source: UnsafePointer<CChar>,
    _ sourceLen: Int,
    _ filePath: UnsafePointer<CChar>
) -> UnsafePointer<CChar>? {
    return nil // Phase 4
}

// MARK: - Memory

@c(grapha_free_string)
public func freeString(_ ptr: UnsafeMutablePointer<CChar>) {
    free(ptr)
}

@c(grapha_free_buffer)
public func freeBuffer(_ ptr: UnsafeMutableRawPointer) {
    free(ptr)
}
```

- [ ] **Step 2: Verify Swift compiles**

Run: `cd grapha-swift/swift-bridge && swift build -c release 2>&1 | tail -5`
Expected: may fail until IndexStoreReader.extractFile return type is updated (Task 5). That's expected — proceed to Task 5.

- [ ] **Step 3: Commit**

```bash
git add grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/Bridge.swift
git commit -m "feat(swift): update FFI exports for binary buffer format"
```

---

### Task 5: Swift Binary Buffer Encoder

**Files:**
- Modify: `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/IndexStoreReader.swift`

This is the largest change. Replace `buildJSON`/`appendEscaped` with `buildBinaryBuffer`, update `ExtractedNode`/`ExtractedEdge` to use u8 enums, and change `extractFile` return type.

- [ ] **Step 1: Change `ExtractedNode.kind` and `ExtractedEdge.kind` from `String` to `UInt8`; change `ExtractedNode.visibility` from `String` to `UInt8`; change `ExtractedEdge.confidence` from `Double` to `UInt8`**

In `IndexStoreReader.swift`, replace the `ExtractedNode` struct:

```swift
private struct ExtractedNode {
    let id: String
    let kind: UInt8
    let name: String
    let file: String
    let line: UInt32
    let col: UInt32
    let visibility: UInt8
    let module: String?
}
```

Replace the `ExtractedEdge` struct:

```swift
private struct ExtractedEdge: Hashable {
    let source: String
    let target: String
    let kind: UInt8
    let confidencePct: UInt8

    func hash(into hasher: inout Hasher) {
        hasher.combine(source)
        hasher.combine(target)
        hasher.combine(kind)
    }

    static func == (lhs: ExtractedEdge, rhs: ExtractedEdge) -> Bool {
        lhs.source == rhs.source && lhs.target == rhs.target && lhs.kind == rhs.kind
    }
}
```

- [ ] **Step 2: Update `mapSymbolKind` to return `UInt8?` instead of `String?`**

```swift
private func mapSymbolKind(_ raw: UInt64) -> UInt8? {
    switch raw {
    case 5:  return 2   // enum
    case 6:  return 1   // struct
    case 7:  return 1   // Class → struct in grapha
    case 8:  return 3   // protocol
    case 9:  return 4   // extension
    case 11: return 5   // type_alias
    case 12: return 0   // Function
    case 13: return 6   // Variable → property
    case 14: return 7   // field
    case 15: return 8   // variant
    case 16: return 0   // InstanceMethod
    case 17: return 0   // ClassMethod
    case 18: return 0   // StaticMethod
    case 19: return 6   // InstanceProperty
    case 20: return 6   // ClassProperty
    case 21: return 6   // StaticProperty
    case 22: return 0   // Constructor
    case 23: return 0   // Destructor
    default: return nil
    }
}
```

- [ ] **Step 3: Add edge kind constants and a confidence helper**

Add after `mapSymbolKind`:

```swift
private struct BinaryEdgeKind {
    static let calls: UInt8 = 0
    static let contains: UInt8 = 1
    static let inherits: UInt8 = 2
    static let implements: UInt8 = 3
    static let typeRef: UInt8 = 4
}
```

- [ ] **Step 4: Update `processOccurrence` to use new types**

In `processOccurrence`, change the `ExtractedNode` creation:

```swift
    if isDefOrDecl, let kind = mapSymbolKind(kindRaw) {
        c.nodes[usr] = ExtractedNode(
            id: usr, kind: kind, name: name, file: c.fileName,
            line: line, col: col, visibility: 0, module: c.moduleName
        )
    }
```

- [ ] **Step 5: Update `extractRelationEdges` to use `UInt8` kinds and confidence**

Replace all edge insertions. For example, change:

```swift
c.edges.insert(ExtractedEdge(
    source: relUSR, target: _cbRelSymbolUSR,
    kind: "calls", confidence: 1.0
))
```

to:

```swift
c.edges.insert(ExtractedEdge(
    source: relUSR, target: _cbRelSymbolUSR,
    kind: BinaryEdgeKind.calls, confidencePct: 100
))
```

Apply the same pattern to all edge insertions:
- `"calls"` / `1.0` → `BinaryEdgeKind.calls` / `100`
- `"contains"` / `1.0` → `BinaryEdgeKind.contains` / `100`
- `"inherits"` / `1.0` → `BinaryEdgeKind.inherits` / `100`
- `"implements"` / `1.0` → `BinaryEdgeKind.implements` / `100`
- `"implements"` / `0.9` → `BinaryEdgeKind.implements` / `90`
- `"type_ref"` / `0.9` → `BinaryEdgeKind.typeRef` / `90`

- [ ] **Step 6: Replace `buildJSON`/`appendEscaped` with `buildBinaryBuffer`**

Remove the `buildJSON` and `appendEscaped` functions. Add:

```swift
private let BINARY_MAGIC: UInt32 = 0x47524148
private let BINARY_VERSION: UInt8 = 1
private let HEADER_SIZE = 20
private let PACKED_NODE_SIZE = 44
private let PACKED_EDGE_SIZE = 20
private let NO_MODULE: UInt32 = 0xFFFFFFFF

private func buildBinaryBuffer(
    nodes: [ExtractedNode],
    edges: Set<ExtractedEdge>
) -> (UnsafeMutableRawPointer, UInt32) {
    // Phase 1: build string table with deduplication
    var stringTable = Data()
    var stringIndex: [String: (UInt32, UInt32)] = [:]

    func intern(_ s: String) -> (UInt32, UInt32) {
        if let existing = stringIndex[s] { return existing }
        let offset = UInt32(stringTable.count)
        let bytes = Array(s.utf8)
        stringTable.append(contentsOf: bytes)
        let entry = (offset, UInt32(bytes.count))
        stringIndex[s] = entry
        return entry
    }

    // Pre-intern all strings (nodes first, then edges reuse via dedup)
    var nodeRefs: [(id: (UInt32, UInt32), name: (UInt32, UInt32), file: (UInt32, UInt32), module: (UInt32, UInt32)?)] = []
    for n in nodes {
        let idRef = intern(n.id)
        let nameRef = intern(n.name)
        let fileRef = intern(n.file)
        let modRef = n.module.map { intern($0) }
        nodeRefs.append((idRef, nameRef, fileRef, modRef))
    }

    var edgeRefs: [(source: (UInt32, UInt32), target: (UInt32, UInt32))] = []
    let edgeArray = Array(edges)
    for e in edgeArray {
        let srcRef = intern(e.source)
        let tgtRef = intern(e.target)
        edgeRefs.append((srcRef, tgtRef))
    }

    // Phase 2: allocate and write buffer
    let nodeCount = UInt32(nodes.count)
    let edgeCount = UInt32(edgeArray.count)
    let strTableOffset = UInt32(HEADER_SIZE + nodes.count * PACKED_NODE_SIZE + edgeArray.count * PACKED_EDGE_SIZE)
    let totalSize = Int(strTableOffset) + stringTable.count

    let buf = malloc(totalSize)!  // must use malloc — freed via free() across FFI
    var pos = 0

    func writeU32(_ val: UInt32) {
        buf.storeBytes(of: val.littleEndian, toByteOffset: pos, as: UInt32.self)
        pos += 4
    }
    func writeU8(_ val: UInt8) {
        buf.storeBytes(of: val, toByteOffset: pos, as: UInt8.self)
        pos += 1
    }
    func pad(_ count: Int) {
        for _ in 0..<count { writeU8(0) }
    }

    // Header
    writeU32(BINARY_MAGIC)
    writeU8(BINARY_VERSION)
    pad(3)
    writeU32(nodeCount)
    writeU32(edgeCount)
    writeU32(strTableOffset)

    // Nodes
    for (i, n) in nodes.enumerated() {
        let refs = nodeRefs[i]
        writeU32(refs.id.0); writeU32(refs.id.1)
        writeU32(refs.name.0); writeU32(refs.name.1)
        writeU32(refs.file.0); writeU32(refs.file.1)
        if let m = refs.module {
            writeU32(m.0); writeU32(m.1)
        } else {
            writeU32(NO_MODULE); writeU32(0)
        }
        writeU32(n.line)
        writeU32(n.col)
        writeU8(n.kind)
        writeU8(n.visibility)
        pad(2)
    }

    // Edges
    for (i, e) in edgeArray.enumerated() {
        let refs = edgeRefs[i]
        writeU32(refs.source.0); writeU32(refs.source.1)
        writeU32(refs.target.0); writeU32(refs.target.1)
        writeU8(e.kind)
        writeU8(e.confidencePct)
        pad(2)
    }

    // String table
    stringTable.withUnsafeBytes { rawBuf in
        buf.advanced(by: Int(strTableOffset)).copyMemory(
            from: rawBuf.baseAddress!,
            byteCount: stringTable.count
        )
    }

    return (buf, UInt32(totalSize))
}
```

- [ ] **Step 7: Update `extractFile` to return binary buffer**

Change the `extractFile` method signature and body:

```swift
    func extractFile(_ filePath: String) -> (UnsafeMutableRawPointer, UInt32)? {
        return _cbLock.withLock { _ -> (UnsafeMutableRawPointer, UInt32)? in

        if fileIndex == nil {
            fileIndex = buildFileIndex()
        }

        let resolved = resolvePath(filePath)
        let fileName = URL(fileURLWithPath: filePath).lastPathComponent

        let unitInfo = fileIndex?[resolved] ?? findByFileName(fileName)

        guard let unitInfo else { return nil }
        guard let recordName = unitInfo.recordName else { return nil }

        let collector = readOccurrences(
            recordName: recordName,
            fileName: fileName,
            moduleName: unitInfo.moduleName
        )

        return buildBinaryBuffer(nodes: Array(collector.nodes.values), edges: collector.edges)
        }
    }
```

- [ ] **Step 8: Verify Swift compiles**

Run: `cd grapha-swift/swift-bridge && swift build -c release 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 9: Commit**

```bash
git add grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/IndexStoreReader.swift
git commit -m "feat(swift): replace JSON encoder with binary buffer encoder"
```

---

### Task 6: End-to-End Build Verification

**Files:** None (verification only)

- [ ] **Step 1: Run Rust tests**

Run: `cargo test -p grapha-swift`
Expected: all binary parser tests pass

- [ ] **Step 2: Full workspace build**

Run: `cargo build`
Expected: workspace builds with the Swift bridge auto-compiled by `build.rs`

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all 173+ tests pass

- [ ] **Step 4: Smoke test with real project**

Run: `cargo run -p grapha -- index <path-to-a-swift-project>`
Expected: successful indexing with nodes/edges output. Compare node/edge counts with the JSON version — they should match exactly.

- [ ] **Step 5: Commit any fixups**

If any issues are found, fix and commit with descriptive messages.

---

### Task 7: Clean Up Dead JSON Code

**Files:**
- Modify: `grapha-swift/Cargo.toml` (optional — remove `serde_json` if only used for indexstore)

- [ ] **Step 1: Check if `serde_json` is still used elsewhere in grapha-swift**

Run: `grep -r "serde_json" grapha-swift/src/`
If only `indexstore.rs` used it and that's gone, remove from `Cargo.toml`.

- [ ] **Step 2: Remove unused dependency if applicable**

In `grapha-swift/Cargo.toml`, remove `serde_json` from `[dependencies]` if unused.

- [ ] **Step 3: Verify build**

Run: `cargo build -p grapha-swift`
Expected: compiles without serde_json

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore(swift): remove unused serde_json dependency"
```
