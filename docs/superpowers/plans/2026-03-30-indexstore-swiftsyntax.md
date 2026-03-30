# Index Store + SwiftSyntax Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill in the Swift bridge stubs to read Xcode's index store (compiler-resolved symbols) and parse Swift via SwiftSyntax (accurate AST), completing the waterfall: index-store → SwiftSyntax → tree-sitter.

**Architecture:** The Swift bridge dylib (`libGraphaSwiftBridge.dylib`) uses libIndexStore's C API to read pre-built Xcode index data and SwiftSyntax's visitor pattern to parse unindexed files. Both return JSON matching `ExtractionResult`. The Rust side (`grapha-swift`) calls these via `dlopen`'d function pointers.

**Tech Stack:** Swift 6.x, SwiftSyntax, libIndexStore.dylib (C API), libloading (Rust), serde_json

---

## Task 1: Implement IndexStoreReader.swift

**Files:**
- Create: `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/IndexStoreReader.swift`
- Modify: `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/Bridge.swift`
- Modify: `grapha-swift/swift-bridge/Package.swift` (add linker flag for libIndexStore)

The index store reader:
1. Opens the DataStore path via `indexstore_store_create`
2. Iterates units to find the record for a given source file
3. Reads the record's symbols and occurrences
4. Maps to JSON matching ExtractionResult format

Key: use `dlopen` to load libIndexStore.dylib from the Swift side (avoiding link-time dependency). The C API uses callback-based iteration (`*_apply_f` variants take a context pointer + function pointer).

The JSON output must match the `ExtractionResult` format from grapha-core:
```json
{
  "nodes": [{ "id": "<USR>", "kind": "function", "name": "foo", "file": "...", "span": {...}, "visibility": "public", "metadata": {}, "module": "MyModule" }],
  "edges": [{ "source": "<caller USR>", "target": "<callee USR>", "kind": "calls", "confidence": 1.0 }],
  "imports": [{ "path": "import Foundation", "symbols": [], "kind": "module" }]
}
```

## Task 2: Wire index store into Rust side

**Files:**
- Modify: `grapha-swift/src/indexstore.rs`
- Modify: `grapha-swift/src/lib.rs`

Implement `extract_from_indexstore()`:
1. Call `bridge.indexstore_open(store_path)` → handle
2. Call `bridge.indexstore_extract(handle, file_path)` → JSON string
3. Parse JSON into `ExtractionResult` via serde
4. Call `bridge.free_string` to release the JSON
5. Return the result

Add index store path auto-discovery in lib.rs:
- Check `~/Library/Developer/Xcode/DerivedData/<project>-*/Index.noindex/DataStore`
- Cache discovered path in a `OnceLock`

## Task 3: Implement SwiftSyntaxExtractor.swift

**Files:**
- Create: `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/SwiftSyntaxExtractor.swift`
- Modify: `grapha-swift/swift-bridge/Sources/GraphaSwiftBridge/Bridge.swift`

SwiftSyntax visitor that:
1. Parses source with `Parser.parse(source:)`
2. Walks AST with `SyntaxVisitor` subclass
3. Visits: ClassDeclSyntax, StructDeclSyntax, FunctionDeclSyntax, InitializerDeclSyntax, DeinitializerDeclSyntax, VariableDeclSyntax, ProtocolDeclSyntax, EnumDeclSyntax, ExtensionDeclSyntax
4. For expressions: FunctionCallExprSyntax, MemberAccessExprSyntax
5. Tracks parent context (current function/type) for edge source
6. Returns JSON matching ExtractionResult

## Task 4: Wire SwiftSyntax into Rust side

**Files:**
- Modify: `grapha-swift/src/swiftsyntax.rs`

Implement `extract_with_swiftsyntax()`:
1. Call `bridge.swiftsyntax_extract(source_ptr, source_len, file_path_ptr)` → JSON string
2. Parse JSON into `ExtractionResult`
3. Call `bridge.free_string`
4. Return result

## Task 5: Integration test on lama-ludo-ios

**Files:**
- Test with real project

Test the full waterfall:
1. Build the Swift bridge: `cargo build -p grapha-swift`
2. Index with index store: point to DerivedData
3. Verify: `grapha impact activityGiftConfigs` shows compiler-resolved results
4. Verify: new/unbuilt files fall back to SwiftSyntax then tree-sitter
