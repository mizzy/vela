# Vela Phase 2: DFE + DCE Improvement — Design Spec

## Overview

Phase 2 adds Duplicate Function Elimination (DFE) and improves Dead Code Elimination (DCE) to fully remove functions rather than replacing bodies with `unreachable`. Both passes share a new index renumbering infrastructure that rewrites all function index references after function deletion.

## Background

Phase 1 DCE replaces dead function bodies with `unreachable` but keeps functions in place to avoid index renumbering. This limits size reduction — AWSCC 8.3MB achieved only 450KB (5.3%) reduction. By fully deleting functions and renumbering indices, we can achieve significantly greater savings.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| DFE strategy | Full deletion + index renumbering | Future-proof; avoids intermediate "body replacement" compromise |
| DCE improvement | Full deletion (replaces Phase 1 body replacement) | Same renumbering infrastructure; no reason to keep compromise |
| Scope | DFE + DCE improvement in one phase | Shared renumbering infrastructure makes it efficient |
| Duplicate detection | `(type_index, body_hash)` key | Body-only match is almost always sufficient, but type_index adds zero-cost safety margin |
| Hash function | `std::hash::DefaultHasher` | Non-cryptographic, fast; collision fallback via full byte comparison |
| Index renumbering | Custom `Reencode` trait implementation | `function_index()` override auto-rewrites all references (call, export, elem, ref.func) |
| Pass order | DCE first, then DFE on reachable functions | Dead functions don't need duplicate detection |

## Architecture

### New files

| File | Responsibility |
|------|---------------|
| `crates/vela-core/src/renumber.rs` | Index renumbering: `FunctionRenumberer`, `build_index_map`, `rebuild_module` |
| `crates/vela-core/src/dfe.rs` | Duplicate detection: `find_duplicates`, `DfeResult` |

### Modified files

| File | Changes |
|------|---------|
| `crates/vela-core/src/dce.rs` | Remove `eliminate_dead_code` (module rebuild logic moves to renumber.rs) |
| `crates/vela-core/src/lib.rs` | Add `dfe` field to `OptimizeConfig`; new `optimize_module` combining DCE + DFE + renumbering |
| `crates/vela-cli/src/main.rs` | Add `--no-dfe` flag |

## Index Renumbering Infrastructure

### `FunctionRenumberer`

A custom `Reencode` implementation that overrides `function_index()` to apply old→new index mapping:

```rust
pub struct FunctionRenumberer {
    index_map: Vec<u32>,  // old_index → new_index
}

impl Reencode for FunctionRenumberer {
    fn function_index(&mut self, idx: u32) -> u32 {
        self.index_map[idx as usize]
    }
    // Other methods delegate to RoundtripReencoder behavior
}
```

The `Reencode` trait automatically applies `function_index()` to all function references: `call` instructions, `return_call`, `ref.func`, export section, elem section, start section.

### `build_index_map`

```rust
pub fn build_index_map(
    num_functions: u32,
    redirects: &HashMap<u32, u32>,  // DFE: duplicate → representative
    removals: &HashSet<u32>,         // functions to delete
) -> Vec<u32>
```

Two-phase mapping:
1. Apply redirects: if function A is a duplicate of B, A's index maps to B's index
2. Compact: removed functions are skipped, remaining functions get sequential indices

### `rebuild_module`

```rust
pub fn rebuild_module(
    module_bytes: &[u8],
    index_map: &[u32],
    removals: &HashSet<u32>,
) -> Result<Vec<u8>, VelaError>
```

Re-encodes the module using `FunctionRenumberer`:
- Function section: skip entries for removed functions
- Code section: skip bodies for removed functions
- All other sections: `FunctionRenumberer` auto-rewrites indices

This replaces the module rebuild logic currently in `dce.rs::eliminate_dead_code`.

## DFE Algorithm

### `find_duplicates`

```rust
pub struct DfeResult {
    pub redirects: HashMap<u32, u32>,  // duplicate → representative
    pub removals: HashSet<u32>,         // same as redirects.keys()
}

pub fn find_duplicates(
    module_bytes: &[u8],
    reachable: &HashSet<u32>,
) -> Result<DfeResult, VelaError>
```

1. Parse code section, collect each function's raw body bytes
2. For each reachable defined function, compute key: `(type_index, hash(body_bytes))`
3. Group by key. For hash collisions, verify full byte equality
4. First function in each group is the representative; others are duplicates
5. Return redirects (duplicate → representative) and removals (duplicate set)

Import functions are skipped (no body).

## Integrated Optimization Pipeline

```rust
fn optimize_module(module_bytes: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    let graph = CallGraph::from_module(module_bytes)?;
    let roots = find_roots(module_bytes, &graph)?;
    let reachable = find_reachable(&roots, &graph);

    // DCE: collect unreachable functions for removal
    let mut removals: HashSet<u32> = (0..graph.num_functions)
        .filter(|i| !reachable.contains(i))
        .collect();

    // DFE: detect duplicates among reachable functions
    let mut redirects = HashMap::new();
    if config.dfe {
        let dfe_result = find_duplicates(module_bytes, &reachable)?;
        redirects = dfe_result.redirects;
        removals.extend(dfe_result.removals);
    }

    // Skip rebuild if nothing to remove
    if removals.is_empty() && redirects.is_empty() {
        return Ok(module_bytes.to_vec());
    }

    // Renumber and rebuild
    let index_map = build_index_map(graph.num_functions, &redirects, &removals);
    rebuild_module(module_bytes, &index_map, &removals)
}
```

If both DCE and DFE are disabled, module bytes are returned unchanged.

## Public API Changes

```rust
pub struct OptimizeConfig {
    pub dce: bool,
    pub dfe: bool,  // new
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { dce: true, dfe: true }
    }
}
```

## CLI Changes

```
vela optimize input.wasm -o output.wasm           # DCE + DFE (default)
vela optimize --no-dfe input.wasm -o output.wasm   # DCE only
vela optimize --no-dce input.wasm -o output.wasm   # DFE only
```

## Test Strategy

| Layer | Content |
|-------|---------|
| `renumber.rs` unit | `build_index_map` correctness: redirects, removals, combined |
| `renumber.rs` integration | `rebuild_module` produces valid WASM after function deletion |
| DCE improvement | Dead functions fully removed (function count decreases) |
| DFE unit | `find_duplicates` detects identical `(type_index, body)` pairs |
| DFE integration | Duplicate functions removed, module valid, function count decreases |
| Combined DCE + DFE | Both passes active, wasmtime executes correctly |

Existing Phase 1 tests (`eliminates_dead_function_body`) will be updated to verify function count decreases rather than staying the same.

## Success Criteria

| Metric | Criterion |
|--------|-----------|
| Correctness | Optimized WASM runs correctly in wasmtime |
| DCE improvement | Greater size reduction than Phase 1 (functions fully removed) |
| DFE reduction | Measurable additional reduction on AWSCC 8.3MB |
| Speed | 8.3MB WASM optimized within 1 minute |
| Validity | `wasmparser::Validator` passes on all outputs |
