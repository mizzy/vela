# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Vela

Vela is a Rust-based WASM Component Model optimizer that reduces binary size through three passes:
- **DCE** (Dead Code Elimination): removes unreachable functions via call graph analysis
- **DFE** (Duplicate Function Elimination): merges functions with identical `(type_index, body_bytes)`
- **RUME** (Remove Unused Module Elements): removes unused tables, memories, globals, and imports

Built for Carina provider plugins (8-11MB WASM Component Model binaries). Achieves ~10% reduction in <0.2s.

## Build & Test Commands

```bash
cargo build                                    # debug build
cargo build --release                          # release build
cargo test                                     # all tests (unit + integration)
cargo test -p vela-core                        # library tests only
cargo test -p vela-core dce                    # tests matching "dce"
cargo test -p vela-core --test integration_test  # integration tests only (uses wasmtime)
```

## CLI Usage

```bash
vela optimize input.wasm -o output.wasm        # all passes (DCE+DFE+RUME)
vela optimize --no-dfe --no-rume input.wasm -o output.wasm  # DCE only
vela info input.wasm                           # show WASM info
```

## Architecture

Workspace with two crates: `vela-core` (library) and `vela-cli` (thin CLI wrapper).

### Optimization Pipeline

`optimize()` → `process_component()` → `optimize_module()` per core module:

1. **CallGraph** (`callgraph.rs`): parse function bodies for `call`/`return_call`/`ref.func`
2. **DCE** (`dce.rs`): `find_roots` (exports, start, elem, imports) → `find_reachable` (BFS) → unreachable functions marked for removal
3. **DFE** (`dfe.rs`): hash `(type_index, body_bytes)` of reachable functions → duplicates redirected to representative → marked for removal
4. **RUME** (`rume.rs`): scan reachable function bodies for table/memory/global usage → unused elements marked for removal
5. **Renumber** (`renumber.rs`): `build_index_map` (redirect → compact) → `ModuleRenumberer` (custom `Reencode` trait impl) → `rebuild_module` (skip removed entries, auto-rewrite all index references)

### Component Model Handling

`component.rs`: `process_component()` extracts core modules from Component WASM, passes each to `optimize_module`, then reconstructs the component. Component metadata (instantiate, canon lift) uses name-based references, so core module index changes don't require component-level rewriting.

### Key Types

- `ModuleRenumberer`: implements `wasm_encoder::reencode::Reencode` trait, overrides `function_index()`, `table_index()`, `memory_index()`, `global_index()` to auto-rewrite all references during re-encoding
- `Removals`: per-index-space sets of indices to delete
- `build_index_map`: two-phase mapping (apply redirects, then compact)

### Known Limitations

- `call_indirect` targets are handled conservatively (all elem-section functions are roots)
- Atomic/SIMD memory instructions are not tracked in RUME usage analysis
- Re-encoding may produce slightly different LEB128 encodings (documented in `rebuild_module`)

## wasmparser/wasm-encoder API Notes (v0.246)

These are version-specific behaviors that differ from documentation:
- `ImportSection` reader needs `.into_imports()` to iterate
- `wasm_encoder::ValType` does NOT implement `From<wasmparser::ValType>` — use `wasm_encoder::ValType::I32` directly
- `wasm_encoder::ModuleSection` takes `&Module` (not `&[u8]`) — for raw bytes use `RawSection { id: ComponentSectionId::CoreModule.into(), data: &bytes }`
- `wasm-encoder` needs `features = ["wasmparser"]` for the `reencode` module
- `wasmtime` needs `features = ["component-model"]` for Component Model support
- `Reencode` trait methods return `Result<T, Error<Self::Error>>`, not plain `T`
