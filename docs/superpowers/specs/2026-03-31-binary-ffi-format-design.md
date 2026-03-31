# Binary FFI Format for Swift-Rust Bridge

**Date:** 2026-03-31
**Status:** Approved
**Scope:** Replace JSON serialization/deserialization in the IndexStore FFI bridge with a flat binary buffer format

## Problem

The current FFI path between Swift and Rust uses JSON:

```
Swift (IndexStore C API) → build JSON string → strdup → FFI → serde_json::from_str → ExtractionResult
```

For a 1991-file project producing 123K nodes and 766K edges, this means:
- Swift builds ~1991 JSON strings with manual string concatenation
- Each string is `strdup`'d across the FFI boundary
- Rust parses each with `serde_json::from_str` (allocates, validates UTF-8, builds serde Value tree, deserializes)

JSON serialization + deserialization is the largest remaining overhead after the IndexStore C API calls themselves.

## Solution

Replace JSON with a flat binary buffer: a single `malloc`'d block containing a header, packed struct arrays, and a string table. Rust reads the buffer with offset arithmetic — zero parsing, zero intermediate allocations.

## Binary Buffer Layout

All integers are **little-endian**. The buffer is a single contiguous allocation with three regions:

```
┌─────────────────────────────────────────────┐
│ Header (20 bytes)                           │
├─────────────────────────────────────────────┤
│ PackedNode[0..N]  (44 bytes each)           │
├─────────────────────────────────────────────┤
│ PackedEdge[0..M]  (20 bytes each)           │
├─────────────────────────────────────────────┤
│ String Table (raw UTF-8, no terminators)    │
└─────────────────────────────────────────────┘
```

### Header (20 bytes)

| Offset | Size | Field                | Description                          |
|--------|------|----------------------|--------------------------------------|
| 0      | 4    | `magic`              | `0x47524148` ("GRAH" in ASCII)       |
| 4      | 1    | `version`            | `1`                                  |
| 5      | 3    | `_pad`               | Reserved, zeroed                     |
| 8      | 4    | `node_count`         | Number of PackedNode entries         |
| 12     | 4    | `edge_count`         | Number of PackedEdge entries         |
| 16     | 4    | `string_table_offset`| Byte offset from buffer start        |

### PackedNode (44 bytes each)

| Offset | Size | Field            | Description                                |
|--------|------|------------------|--------------------------------------------|
| 0      | 4    | `id_offset`      | String table offset for node ID (USR)      |
| 4      | 4    | `id_len`         | Length in bytes                             |
| 8      | 4    | `name_offset`    | String table offset for symbol name        |
| 12     | 4    | `name_len`       | Length in bytes                             |
| 16     | 4    | `file_offset`    | String table offset for file name          |
| 20     | 4    | `file_len`       | Length in bytes                             |
| 24     | 4    | `module_offset`  | String table offset (`0xFFFFFFFF` = none)  |
| 28     | 4    | `module_len`     | Length in bytes (0 if no module)            |
| 32     | 4    | `line`           | Source line number                          |
| 36     | 4    | `col`            | Source column number                        |
| 40     | 1    | `kind`           | NodeKind enum (see table below)            |
| 41     | 1    | `visibility`     | Visibility enum (see table below)          |
| 42     | 2    | `_pad`           | Reserved, zeroed                           |

### PackedEdge (20 bytes each)

| Offset | Size | Field              | Description                              |
|--------|------|--------------------|------------------------------------------|
| 0      | 4    | `source_offset`    | String table offset for source USR       |
| 4      | 4    | `source_len`       | Length in bytes                           |
| 8      | 4    | `target_offset`    | String table offset for target USR       |
| 12     | 4    | `target_len`       | Length in bytes                           |
| 16     | 1    | `kind`             | EdgeKind enum (see table below)          |
| 17     | 1    | `confidence_pct`   | 0-100 (divide by 100.0 on Rust side)    |
| 18     | 2    | `_pad`             | Reserved, zeroed                         |

### String Table

Raw UTF-8 bytes, no null terminators. Each string is referenced by an `(offset, len)` pair in the packed structs. Offsets are relative to the start of the string table (i.e., `buffer[string_table_offset + str_offset]`).

### Enum Encodings

**NodeKind** (u8):

| Value | Kind        |
|-------|-------------|
| 0     | function    |
| 1     | struct      |
| 2     | enum        |
| 3     | protocol    |
| 4     | extension   |
| 5     | type_alias  |
| 6     | property    |
| 7     | field       |
| 8     | variant     |

**EdgeKind** (u8):

| Value | Kind       |
|-------|------------|
| 0     | calls      |
| 1     | contains   |
| 2     | inherits   |
| 3     | implements |
| 4     | type_ref   |

**Visibility** (u8):

| Value | Visibility |
|-------|------------|
| 0     | public     |
| 1     | internal   |
| 2     | private    |

## String Deduplication

USR strings appear as node `id` and repeat as edge `source`/`target`. The Swift encoder maintains a `Dictionary<String, (UInt32, UInt32)>` mapping strings to their `(offset, len)` in the string table. When a string is encountered again, its existing offset is reused. This can halve the string table size for typical files.

## FFI Signature Change

### Old

```c
// Swift → Rust: JSON C string
const char* grapha_indexstore_extract(void* handle, const char* filePath);
void grapha_free_string(char* ptr);
```

### New

```c
// Swift → Rust: binary buffer + length via out-parameter
const uint8_t* grapha_indexstore_extract(void* handle, const char* filePath, uint32_t* outLen);
void grapha_free_buffer(uint8_t* ptr);  // same as free(), kept for semantic clarity
```

The old `grapha_free_string` is retained for any remaining string-based FFI (e.g., future SwiftSyntax). A new `grapha_free_buffer` is added for the binary buffer (both call `free()`).

## Swift-Side Changes

### Bridge.swift

- Update `indexstoreExtract` signature: add `outLen: UnsafeMutablePointer<UInt32>` parameter, return `UnsafeRawPointer?` instead of `UnsafePointer<CChar>?`
- Add `grapha_free_buffer` export (calls `free()`)

### IndexStoreReader.swift

- `extractFile` returns `(UnsafeRawPointer, UInt32)?` instead of `String?`
- Remove `buildJSON`, `appendEscaped`
- Add `buildBinaryBuffer(nodes:edges:) -> (UnsafeMutableRawPointer, UInt32)`:
  1. Build string table with dedup dictionary
  2. Calculate total buffer size: `20 + 44*N + 20*M + string_table_size`
  3. `malloc` the buffer
  4. Write header, packed nodes, packed edges, string table
  5. Return `(pointer, total_size)`

## Rust-Side Changes

### bridge.rs

- Update `IndexStoreExtractFn` type: `fn(*mut c_void, *const i8, *mut u32) -> *const u8`
- Add `FreeBufferFn` type and load `grapha_free_buffer` symbol

### indexstore.rs

- Replace `serde_json::from_str` with `parse_binary_buffer`:
  1. Read header (20 bytes): validate magic `0x47524148`, version `1`
  2. Read `node_count`, `edge_count`, `string_table_offset`
  3. Iterate PackedNode array at offset 20, building `Vec<Node>`
  4. Iterate PackedEdge array at offset `20 + 44*N`, building `Vec<Edge>`
  5. For each string ref: slice `buffer[string_table_offset + offset .. + offset + len]`, convert via `std::str::from_utf8`
  6. Return `ExtractionResult { nodes, edges, imports: vec![] }`

## What Doesn't Change

- `grapha_indexstore_open` / `grapha_indexstore_close` signatures
- Waterfall logic in `lib.rs` (tree-sitter fallback returns `ExtractionResult` directly)
- `ExtractionResult` struct in `grapha-core`
- All downstream consumers (query engines, persistence, etc.)

## Size Estimate

For a typical file (~60 nodes, ~400 edges):

| Format | Estimated Size |
|--------|---------------|
| JSON   | ~40 KB        |
| Binary | ~15 KB        |
| Binary (with string dedup) | ~10 KB |

## Validation

- Magic number prevents misinterpreting garbage as valid data
- Version byte enables future format evolution without breaking existing readers
- Rust parser validates all string offsets are within the string table bounds
- Rust parser validates UTF-8 for all extracted strings (`from_utf8`, not `from_utf8_unchecked`)

## Error Handling

- If `extractFile` fails (no unit info, no record name), it returns `nil` — same as before
- If the Rust parser encounters an invalid buffer (bad magic, out-of-bounds offsets, invalid UTF-8), it returns `None` — same error path as JSON parse failure
- No partial results: a file either succeeds completely or returns `None`
