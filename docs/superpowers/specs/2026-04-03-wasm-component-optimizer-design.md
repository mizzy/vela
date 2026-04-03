# Vela: WASM Component Model Optimizer — Design Spec

## Overview

Vela is a Rust-based WASM Component Model optimizer that reduces binary size and improves compilation time for Carina provider plugins. It replaces the MoonBit-based `wite` tool, which cannot handle large (8MB+) WASM files in practical time due to MoonBit's byte operation overhead (copy-on-slice, GC pressure).

## Background

Carina provider plugins are built as WASM Component Model binaries (8.3MB-11MB). Investigation of `wite` revealed:

- MoonBit's `Bytes` slicing creates O(n) copies, making parse/rebuild cycles extremely slow
- 8.3MB WASM: DFE takes 29 min, DCE 17+ min (did not complete), full -O1 60+ min
- Rust's `&[u8]` zero-copy slicing and `memcpy`-backed buffer operations would be orders of magnitude faster

Full investigation details are documented in the wasm-optimizer-handoff document (originally at `/tmp/wasm-optimizer-handoff.md`).

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | wasmparser/wasm-encoder ecosystem, zero-copy `&[u8]`, Carina stack unity |
| Approach | wasmparser + wasm-encoder, self-built | No existing Component Model optimizer; existing tools (walrus, wasm-shrink) are core-module-only or wrong purpose |
| Integration | CLI tool for build-time use | Runtime optimization adds latency; build-time via CI/CD is simplest |
| Project structure | Workspace: `vela-core` (lib) + `vela-cli` (bin) | Testability, reusability; standard Rust pattern |
| Phased rollout | Phase 1: DCE → Phase 2: DFE → Phase 3: RUME | Incremental, each phase builds on prior infrastructure |
| Initial scope | Phase 1 (DCE) only | Largest impact, establishes parsing/testing foundation |

## Project Structure

```
vela/
├── Cargo.toml              # workspace root
├── crates/
│   ├── vela-core/
│   │   ├── Cargo.toml      # wasmparser, wasm-encoder
│   │   └── src/
│   │       ├── lib.rs       # Public API: optimize(bytes, config) -> Result<Vec<u8>>
│   │       ├── component.rs # Component Model WASM traversal and reconstruction
│   │       ├── callgraph.rs # Call graph construction
│   │       ├── dce.rs       # Dead Code Elimination
│   │       └── error.rs     # Error types
│   └── vela-cli/
│       ├── Cargo.toml       # vela-core, clap
│       └── src/
│           └── main.rs      # CLI entry point
├── tests/
│   └── integration/         # Integration tests
└── benches/
    ├── fetch_fixtures.sh    # Download large WASM for benchmarks
    ├── fixtures/            # .gitignore'd
    └── optimize_bench.rs    # criterion benchmarks
```

## Public API

```rust
pub struct OptimizeConfig {
    pub dce: bool,
    // Phase 2: pub dfe: bool,
    // Phase 3: pub rume: bool,
}

pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError>;
```

## CLI

```
vela optimize input.wasm -o output.wasm       # Default: DCE enabled
vela optimize --no-dce input.wasm -o output.wasm
vela info input.wasm                           # Show WASM info (size, function count, etc.)
```

## Component Model Processing Flow

Component Model WASM has a nested structure:

```
Component (top level)
├── core module 0 (contains actual code)
├── core module 1 (if multiple modules)
├── component metadata (import/export definitions, type info)
└── custom sections (names, producers, etc.)
```

Processing steps:

1. **Parse**: Stream-parse Component WASM with `wasmparser::Parser::new(0)`, walk `Payload` variants
2. **Extract core modules**: Collect each core module's bytes from `Payload::ModuleSection`
3. **Optimize each core module independently**: Parse and apply DCE per core module
4. **Reconstruct**: Rebuild Component with wasm-encoder, substituting optimized core modules; copy all other sections (component metadata, custom sections) unchanged

**Key constraint**: Component metadata references core module imports/exports by index. Exported functions are treated as roots and never removed.

## DCE Algorithm

Per core module:

### Step 1: Call Graph Construction

Parse code section with wasmparser, scan each function body for `call` and `call_indirect` instructions to build a call graph.

```rust
struct CallGraph {
    edges: HashMap<u32, HashSet<u32>>,
}
```

### Step 2: Root Function Identification

Mark as root (non-removable):
- Exported functions (referenced by component metadata)
- Start function (module entry point)
- Functions in `elem` section (stored in tables, reachable via `call_indirect`)
- Imported functions (no body, but occupy index space)

### Step 3: Reachability Analysis

BFS/DFS from roots through call graph. Unmarked functions are dead code.

### Step 4: Reconstruction

Build new core module with wasm-encoder:
- Replace dead function bodies with a single `unreachable` instruction
- Do NOT delete functions — this preserves index integrity across all `call` instructions, exports, and elem sections
- Future improvement: implement index renumbering to fully remove dead functions for additional size savings

## Error Handling

```rust
pub enum VelaError {
    InvalidWasm(String),
    NotComponent(String),
    Wasm(wasmparser::BinaryReaderError),
    Io(std::io::Error),
}
```

On failure, return an error — never silently output a potentially broken WASM.

## Test Strategy

| Layer | Content | WASM Source |
|-------|---------|-------------|
| Unit tests | Call graph construction, reachability analysis logic | Dynamically generated with wasm-encoder + wit-component |
| Integration tests | Optimize Component WASM → load and execute with wasmtime | Dynamically generated fixtures |
| Benchmarks | Measure optimization time on real-world large WASM | On-demand download via `fetch_fixtures.sh` |

Test WASM fixtures are generated programmatically in test code using wasm-encoder and wit-component. This avoids committing binary files and allows precise control over test scenarios (e.g., "module with 3 dead functions").

Integration tests instantiate both pre- and post-optimization WASM with wasmtime and verify exported functions return identical results.

## Success Criteria (Phase 1: DCE)

| Metric | Criterion |
|--------|-----------|
| Correctness | Optimized WASM runs correctly in wasmtime |
| Size reduction | Measurable reduction on MockProvider |
| Speed | DCE on 8.3MB WASM completes within 1 minute |
| Safety | Export/import indices remain intact |

## Future Phases

- **Phase 2: DFE (Duplicate Function Elimination)** — Hash function bodies, merge duplicates. Builds on Phase 1's parsing infrastructure.
- **Phase 3: RUME (Remove Unused Module Elements)** — Remove unused tables, memories, globals. Requires import protection logic (preserve imports referenced by component metadata).
