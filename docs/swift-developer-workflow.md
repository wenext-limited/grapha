# Swift Developer Workflow

Use `grapha-swift` in one of three explicit bridge modes:

- `GRAPHA_SWIFT_BRIDGE_MODE=auto` - default; build the Swift bridge when available, otherwise compile the fallback-only path
- `GRAPHA_SWIFT_BRIDGE_MODE=off` - skip all Swift bridge probing and compile the fallback-only path intentionally
- `GRAPHA_SWIFT_BRIDGE_MODE=required` - require a successful Swift bridge build and fail fast if it cannot be built

## Local Setup

1. Install Xcode and the Swift toolchain.
2. Build the target app in Xcode at least once so DerivedData contains an index store.
3. From the repo root, choose the bridge mode that matches the workflow you want to validate.

## Fast Validation

```bash
cargo test -p grapha-swift --test build_modes
cargo test -p grapha-swift
GRAPHA_SWIFT_BRIDGE_MODE=off cargo test -p grapha-swift
swift test --package-path grapha-swift/swift-bridge
```

## Explicit Build Checks

Use separate target directories so each mode rebuilds with its own build-script output:

```bash
CARGO_TARGET_DIR=target/swift-auto cargo build -p grapha-swift
CARGO_TARGET_DIR=target/swift-off GRAPHA_SWIFT_BRIDGE_MODE=off cargo build -p grapha-swift
CARGO_TARGET_DIR=target/swift-required GRAPHA_SWIFT_BRIDGE_MODE=required cargo build -p grapha-swift
```

## `lama-ludo-ios` Validation

Bridge-on validation run (`required` mode, so bridge failures do not silently fall back):

```bash
GRAPHA_SWIFT_BRIDGE_MODE=required cargo run -p grapha -- index /Users/wendell/Developer/WeNext/lama-ludo-ios --timing
```

Bridge-off run:

```bash
GRAPHA_SWIFT_BRIDGE_MODE=off cargo run -p grapha -- index /Users/wendell/Developer/WeNext/lama-ludo-ios --timing
```

Compare the timing output and confirm both runs finish successfully. The `required` bridge-on run should use the index-store and SwiftSyntax stages when DerivedData is present; the bridge-off run should stay on the fallback path without probing the Swift bridge.
