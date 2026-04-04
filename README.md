# Vela

WASM Component Model optimizer for [Carina](https://github.com/carina-rs/carina) provider plugins.

Reduces binary size through three optimization passes:

- **DCE** — Dead Code Elimination: removes unreachable functions via call graph analysis
- **DFE** — Duplicate Function Elimination: merges functions with identical type and body
- **RUME** — Remove Unused Module Elements: removes unused tables, memories, globals, and imports

## Results

Tested on Carina provider plugins:

| Provider | Original | Optimized | Reduction | Time |
|----------|----------|-----------|-----------|------|
| AWSCC | 8.3MB | 7.6MB | 729KB (8.6%) | 0.09s |
| AWS | 11.4MB | 10.3MB | 1.1MB (9.7%) | 0.13s |

## Installation

```bash
cargo install --path crates/vela-cli
```

## Usage

```bash
# Optimize a WASM Component Model binary (all passes enabled)
vela optimize input.wasm -o output.wasm

# Show WASM info
vela info input.wasm

# Disable individual passes
vela optimize --no-dfe input.wasm -o output.wasm   # skip DFE
vela optimize --no-rume input.wasm -o output.wasm   # skip RUME
vela optimize --no-dce input.wasm -o output.wasm    # skip DCE
```

## Library Usage

```rust
use vela_core::{optimize, OptimizeConfig};

let wasm = std::fs::read("provider.wasm")?;
let config = OptimizeConfig::default(); // DCE + DFE + RUME
let optimized = optimize(&wasm, &config)?;
std::fs::write("provider-optimized.wasm", &optimized)?;
```

## How It Works

Vela processes WASM Component Model binaries by extracting each core module, optimizing it independently, and reconstructing the component.

For each core module:

1. Build a call graph from function bodies
2. **DCE**: identify root functions (exports, start, elem entries), compute reachability, mark unreachable functions for deletion
3. **DFE**: hash reachable function bodies by `(type_index, body_bytes)`, redirect duplicates to a representative, mark duplicates for deletion
4. **RUME**: scan reachable function bodies for table/memory/global references, mark unused elements for deletion
5. Build index maps for all index spaces (function, table, memory, global), re-encode the module with a custom [`Reencode`](https://docs.rs/wasm-encoder/latest/wasm_encoder/reencode/trait.Reencode.html) implementation that rewrites all index references and skips deleted entries
