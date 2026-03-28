# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Grapha is a lightweight structural abstraction layer that transforms complex source code into a normalized, graph-based representation optimized for LLM consumption. It uses fast syntax parsing (via tree-sitter) to extract symbols, relationships, and call patterns, then compresses them into a navigable node graph. This enables agents to efficiently locate, traverse, and reason about code at scale with minimal context.

## Build & Development Commands

```bash
cargo build                    # Build the project
cargo run -- <args>            # Run the CLI
cargo test                     # Run all tests
cargo test <test_name>         # Run a single test
cargo test --lib               # Run unit tests only
cargo test --test <name>       # Run a specific integration test
cargo clippy                   # Lint
cargo fmt                      # Format code
cargo fmt -- --check           # Check formatting without modifying
```

## Architecture

- **Language**: Rust, CLI-first (using `clap` for argument parsing)
- **Parsing**: tree-sitter grammars for language-specific syntax parsing (starting with Swift via `tree-sitter-swift`)
- **Design principle**: Structural/syntactic analysis only — no compiler-level semantic validation. Full semantic checks are deferred to downstream tooling when necessary.

### Core Pipeline

1. **Parse** — tree-sitter parses source files into concrete syntax trees
2. **Extract** — walk CSTs to identify symbols (functions, types, protocols, properties) and relationships (calls, conformances, imports, inheritance)
3. **Normalize** — transform extracted data into a unified node graph representation independent of source language
4. **Compress** — minimize graph for LLM consumption while preserving navigability
5. **Output** — serialize the graph (JSON or other formats) for agent consumption

### Module Style

Use `foo.rs` + `foo/` directory pattern (not `foo/mod.rs`).

### Key Design Decisions

- Tree-sitter over full compiler frontends: prioritizes speed and language coverage over semantic correctness
- Graph nodes represent symbols; edges represent relationships (calls, inherits, conforms, imports)
- The graph is designed to be traversed by LLM agents — optimize for minimal tokens, not human readability
