# Repo Smells Scope Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--file` and `--symbol` scope options to `grapha repo smells` so users can run local smell analysis for one file or one symbol neighborhood.

**Architecture:** Extend the CLI to accept mutually exclusive smell scopes, add scoped subgraph construction in the smell query layer, and keep existing module/full-repo behavior unchanged. Reuse existing file and symbol resolution utilities so scope selection follows the same matching rules as other repo queries.

**Tech Stack:** Rust, clap, existing `grapha` query/resolution modules, integration tests with `assert_cmd`

---

### Task 1: Add failing integration coverage for new smell scopes

**Files:**
- Modify: `grapha/tests/integration.rs`

- [ ] Add an integration test for `grapha repo smells --file <file>` on a small fixture project.
- [ ] Run the targeted integration test and verify it fails because `--file` is not supported yet.
- [ ] Add an integration test for `grapha repo smells --symbol <symbol>` on a small fixture project.
- [ ] Run the targeted integration test and verify it fails because `--symbol` is not supported yet.

### Task 2: Add CLI flags and scope-aware repo smell dispatch

**Files:**
- Modify: `grapha/src/main.rs`

- [ ] Add `--file` and `--symbol` flags to `RepoCommands::Smells`, keeping them mutually exclusive with `--module`.
- [ ] Update smell command handling to resolve the selected scope and call the scoped query function.
- [ ] Run the targeted integration tests and verify they now fail in the query layer instead of argument parsing.

### Task 3: Implement scope-relative smell analysis

**Files:**
- Modify: `grapha/src/query/smells.rs`
- Modify: `grapha/src/query.rs`

- [ ] Add a scope type for smell analysis that supports full graph, module filter, file scope, and symbol neighborhood scope.
- [ ] Reuse existing file matching and symbol resolution helpers to collect the scoped node set.
- [ ] Build a reduced graph from the scoped nodes plus connecting edges needed for local smell metrics.
- [ ] Run unit tests for the smell query layer and verify the new scope behavior passes.

### Task 4: Verify end-to-end behavior and performance

**Files:**
- Modify: `grapha/tests/integration.rs` if assertions need tightening

- [ ] Run `cargo test -p grapha`.
- [ ] Run `cargo build --release -p grapha`.
- [ ] Run `target/release/grapha repo smells --file <...>` and `--symbol <...>` against a real indexed project and confirm output shape and timing.
