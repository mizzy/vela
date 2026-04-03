# Vela Phase 2: DFE + DCE Improvement — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Duplicate Function Elimination and improve Dead Code Elimination to fully delete functions with index renumbering, significantly increasing binary size reduction.

**Architecture:** A shared `FunctionRenumberer` (custom `Reencode` impl) handles index rewriting. `build_index_map` computes old→new mappings from redirects (DFE) and removals (DCE+DFE). `rebuild_module` re-encodes the module skipping removed functions. The existing `eliminate_dead_code` is replaced by a unified `optimize_module` pipeline: DCE → DFE → renumber → rebuild.

**Tech Stack:** Rust 1.93+, wasmparser 0.246, wasm-encoder 0.246 (reencode feature), clap 4, wasmtime 43 (test only)

---

## File Map

| File | Responsibility |
|------|---------------|
| `crates/vela-core/src/renumber.rs` | `FunctionRenumberer`, `build_index_map`, `rebuild_module` |
| `crates/vela-core/src/dfe.rs` | `find_duplicates`, `DfeResult` |
| `crates/vela-core/src/dce.rs` | Remove `eliminate_dead_code`; keep `find_roots`, `find_reachable` |
| `crates/vela-core/src/lib.rs` | Add `dfe` to `OptimizeConfig`; add `optimize_module` |
| `crates/vela-core/src/testutil.rs` | Add `build_module_with_duplicates` helper |
| `crates/vela-cli/src/main.rs` | Add `--no-dfe` flag |
| `crates/vela-core/tests/integration_test.rs` | Add DFE + combined DCE+DFE wasmtime tests |

---

### Task 1: `build_index_map` — Index Mapping Logic

**Files:**
- Create: `crates/vela-core/src/renumber.rs`
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/vela-core/src/renumber.rs`:

```rust
// crates/vela-core/src/renumber.rs
use std::collections::{HashMap, HashSet};

/// Build a mapping from old function index to new function index.
///
/// Two-phase:
/// 1. Apply redirects (DFE: duplicate → representative)
/// 2. Compact: removed functions are skipped, remaining get sequential indices
pub fn build_index_map(
    num_functions: u32,
    redirects: &HashMap<u32, u32>,
    removals: &HashSet<u32>,
) -> Vec<u32> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removals_only() {
        // 5 functions (0..5), remove func 2 and func 4
        let removals = HashSet::from([2, 4]);
        let map = build_index_map(5, &HashMap::new(), &removals);

        // 0→0, 1→1, 2→dead(ignored), 3→2, 4→dead(ignored)
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1);
        assert_eq!(map[3], 2);
    }

    #[test]
    fn redirects_and_removals() {
        // 4 functions: func 2 is duplicate of func 1, so redirect 2→1, remove 2
        let redirects = HashMap::from([(2, 1)]);
        let removals = HashSet::from([2]);
        let map = build_index_map(4, &redirects, &removals);

        // 0→0, 1→1, 2→1(redirect to 1, then 1 stays at 1), 3→2(compacted)
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1);
        assert_eq!(map[2], 1); // redirected to func 1
        assert_eq!(map[3], 2); // compacted from 3 to 2
    }

    #[test]
    fn no_changes() {
        let map = build_index_map(3, &HashMap::new(), &HashSet::new());
        assert_eq!(map, vec![0, 1, 2]);
    }

    #[test]
    fn redirect_chain_through_removal() {
        // func 1 and func 2 are duplicates of func 0. Remove 1 and 2.
        let redirects = HashMap::from([(1, 0), (2, 0)]);
        let removals = HashSet::from([1, 2]);
        let map = build_index_map(4, &redirects, &removals);

        // 0→0, 1→0(redirect), 2→0(redirect), 3→1(compacted)
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 0);
        assert_eq!(map[2], 0);
        assert_eq!(map[3], 1);
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Add to `crates/vela-core/src/lib.rs` after the existing module declarations:

```rust
pub mod renumber;
```

(Keep all existing modules and code unchanged.)

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p vela-core build_index_map`
Expected: FAIL with `not yet implemented`

- [ ] **Step 4: Implement `build_index_map`**

Replace the `todo!()` in `build_index_map`:

```rust
pub fn build_index_map(
    num_functions: u32,
    redirects: &HashMap<u32, u32>,
    removals: &HashSet<u32>,
) -> Vec<u32> {
    // Phase 1: apply redirects
    let mut map: Vec<u32> = (0..num_functions).collect();
    for (&from, &to) in redirects {
        map[from as usize] = to;
    }

    // Phase 2: compact — compute new indices for non-removed functions
    let mut compact_map: Vec<u32> = vec![0; num_functions as usize];
    let mut next_index: u32 = 0;
    for i in 0..num_functions {
        if !removals.contains(&i) {
            compact_map[i as usize] = next_index;
            next_index += 1;
        }
    }

    // Combine: for each old index, follow redirect then compact
    for i in 0..num_functions as usize {
        map[i] = compact_map[map[i] as usize];
    }

    map
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p vela-core build_index_map`
Expected: All 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/vela-core/src/renumber.rs crates/vela-core/src/lib.rs
git commit -m "feat: build_index_map for function index renumbering"
```

---

### Task 2: `FunctionRenumberer` + `rebuild_module`

**Files:**
- Modify: `crates/vela-core/src/renumber.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/vela-core/src/renumber.rs` — the struct, the function signature with `todo!()`, and a test:

```rust
use crate::error::VelaError;
use wasm_encoder::reencode::{Error, Reencode, RoundtripReencoder};
use std::convert::Infallible;

/// Custom Reencode implementation that remaps function indices.
pub struct FunctionRenumberer {
    index_map: Vec<u32>,
}

impl Reencode for FunctionRenumberer {
    type Error = Infallible;

    fn function_index(&mut self, func: u32) -> Result<u32, Error<Self::Error>> {
        Ok(self.index_map[func as usize])
    }
}

/// Re-encode a core module, removing functions in `removals` and applying
/// index remapping via `index_map`.
pub fn rebuild_module(
    module_bytes: &[u8],
    index_map: &[u32],
    removals: &HashSet<u32>,
    num_imports: u32,
) -> Result<Vec<u8>, VelaError> {
    todo!()
}
```

Add test to the existing `tests` module:

```rust
    #[test]
    fn rebuild_removes_dead_function() {
        use wasm_encoder::*;

        // Build module: func 0 (exported "run", calls func 1), func 1 (leaf), func 2 (dead)
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(0);
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(1));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(999));
        f2.instruction(&Instruction::Drop);
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        module.section(&codes);
        let wasm = module.finish();

        // Remove func 2, renumber: 0→0, 1→1, 2→dead
        let removals = HashSet::from([2]);
        let index_map = build_index_map(3, &HashMap::new(), &removals);
        let rebuilt = rebuild_module(&wasm, &index_map, &removals, 0).expect("rebuild should succeed");

        // Validate
        wasmparser::Validator::new().validate_all(&rebuilt).expect("should be valid");

        // Should have 2 functions now (not 3)
        let graph = crate::callgraph::CallGraph::from_module(&rebuilt).unwrap();
        assert_eq!(graph.num_functions, 2);

        // Should be smaller
        assert!(rebuilt.len() < wasm.len());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vela-core rebuild_removes`
Expected: FAIL with `not yet implemented`

- [ ] **Step 3: Implement `rebuild_module`**

Replace the `todo!()` with:

```rust
pub fn rebuild_module(
    module_bytes: &[u8],
    index_map: &[u32],
    removals: &HashSet<u32>,
    num_imports: u32,
) -> Result<Vec<u8>, VelaError> {
    let enc_err = |e: Error<Infallible>| VelaError::InvalidWasm(format!("{e:?}"));
    let parser = wasmparser::Parser::new(0);
    let mut module = wasm_encoder::Module::new();
    let mut reencoder = FunctionRenumberer {
        index_map: index_map.to_vec(),
    };

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::Version { .. } => {}
            wasmparser::Payload::FunctionSection(reader) => {
                let mut sec = wasm_encoder::FunctionSection::new();
                for (i, type_idx) in reader.into_iter().enumerate() {
                    let func_idx = num_imports + i as u32;
                    if !removals.contains(&func_idx) {
                        sec.function(reencoder.type_index(type_idx?).map_err(&enc_err)?);
                    }
                }
                module.section(&sec);
            }
            wasmparser::Payload::CodeSectionStart { range, .. } => {
                let section_bytes = &module_bytes[range.start..range.end];
                let reader = wasmparser::BinaryReader::new(section_bytes, range.start);
                let code_reader = wasmparser::CodeSectionReader::new(reader)?;

                let mut code_section = wasm_encoder::CodeSection::new();
                for (code_index, func_result) in code_reader.into_iter().enumerate() {
                    let func_body = func_result?;
                    let func_idx = num_imports + code_index as u32;
                    if !removals.contains(&func_idx) {
                        reencoder
                            .parse_function_body(&mut code_section, func_body)
                            .map_err(&enc_err)?;
                    }
                }
                module.section(&code_section);
            }
            wasmparser::Payload::CodeSectionEntry(_) => {}
            wasmparser::Payload::TypeSection(s) => {
                let mut sec = wasm_encoder::TypeSection::new();
                reencoder.parse_type_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::ImportSection(s) => {
                let mut sec = wasm_encoder::ImportSection::new();
                reencoder.parse_import_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::TableSection(s) => {
                let mut sec = wasm_encoder::TableSection::new();
                reencoder.parse_table_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::MemorySection(s) => {
                let mut sec = wasm_encoder::MemorySection::new();
                reencoder.parse_memory_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::TagSection(s) => {
                let mut sec = wasm_encoder::TagSection::new();
                reencoder.parse_tag_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::GlobalSection(s) => {
                let mut sec = wasm_encoder::GlobalSection::new();
                reencoder.parse_global_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::ExportSection(s) => {
                let mut sec = wasm_encoder::ExportSection::new();
                reencoder.parse_export_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::StartSection { func, .. } => {
                module.section(&wasm_encoder::StartSection {
                    function_index: reencoder.function_index(func).map_err(&enc_err)?,
                });
            }
            wasmparser::Payload::ElementSection(s) => {
                let mut sec = wasm_encoder::ElementSection::new();
                reencoder.parse_element_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::DataCountSection { count, .. } => {
                module.section(&wasm_encoder::DataCountSection { count });
            }
            wasmparser::Payload::DataSection(s) => {
                let mut sec = wasm_encoder::DataSection::new();
                reencoder.parse_data_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::CustomSection(s) => {
                reencoder.parse_custom_section(&mut module, s).map_err(&enc_err)?;
            }
            wasmparser::Payload::End(_) => {}
            _ => {}
        }
    }

    Ok(module.finish())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vela-core rebuild_removes`
Expected: PASS

- [ ] **Step 5: Add test for redirect + removal (DFE scenario)**

Add to the tests module:

```rust
    #[test]
    fn rebuild_with_redirect_and_removal() {
        use wasm_encoder::*;

        // func 0: exported "run", calls func 1
        // func 1: returns i32 42
        // func 2: duplicate of func 1 (same body), called by nobody
        //   → redirect 2→1, remove 2
        // func 3: calls func 2 (will be rewritten to call func 1 after redirect)
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0); // func 0: () -> ()
        functions.function(1); // func 1: () -> i32
        functions.function(1); // func 2: () -> i32 (duplicate of func 1)
        functions.function(0); // func 3: () -> ()
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(3));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(42));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        // func 2: same body as func 1
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(42));
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        // func 3: calls func 2
        let mut f3 = Function::new(vec![]);
        f3.instruction(&Instruction::Call(2));
        f3.instruction(&Instruction::Drop);
        f3.instruction(&Instruction::End);
        codes.function(&f3);

        module.section(&codes);
        let wasm = module.finish();

        // Redirect func 2 → func 1, remove func 2
        let redirects = HashMap::from([(2u32, 1u32)]);
        let removals = HashSet::from([2u32]);
        let index_map = build_index_map(4, &redirects, &removals);
        let rebuilt = rebuild_module(&wasm, &index_map, &removals, 0).expect("rebuild should succeed");

        wasmparser::Validator::new().validate_all(&rebuilt).expect("should be valid");

        let graph = crate::callgraph::CallGraph::from_module(&rebuilt).unwrap();
        assert_eq!(graph.num_functions, 3, "should have 3 functions after removing duplicate");

        // func 2 (now index 2, was func 3) should call func 1 (not func 2 which was removed)
        assert!(
            graph.edges.get(&2).map_or(false, |c| c.contains(&1)),
            "func 3 (now 2) should call func 1 after redirect"
        );
    }
```

- [ ] **Step 6: Run all renumber tests**

Run: `cargo test -p vela-core renumber`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/vela-core/src/renumber.rs
git commit -m "feat: FunctionRenumberer and rebuild_module for function deletion"
```

---

### Task 3: DCE Improvement — Replace Body Substitution with Full Deletion

**Files:**
- Modify: `crates/vela-core/src/dce.rs`
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Replace `eliminate_dead_code` with `eliminate_dead_functions`**

In `crates/vela-core/src/dce.rs`, remove the `eliminate_dead_code` function (lines 82-186) and replace it with:

```rust
/// Apply DCE: fully remove unreachable functions and renumber indices.
/// Returns the optimized module bytes.
pub fn eliminate_dead_functions(module_bytes: &[u8]) -> Result<Vec<u8>, VelaError> {
    let graph = CallGraph::from_module(module_bytes)?;
    let roots = find_roots(module_bytes, &graph)?;
    let reachable = find_reachable(&roots, &graph);

    let removals: HashSet<u32> = (0..graph.num_functions)
        .filter(|i| !reachable.contains(i))
        .collect();

    if removals.is_empty() {
        return Ok(module_bytes.to_vec());
    }

    let index_map = crate::renumber::build_index_map(
        graph.num_functions,
        &std::collections::HashMap::new(),
        &removals,
    );
    crate::renumber::rebuild_module(module_bytes, &index_map, &removals, graph.num_imports)
}
```

Also add `use std::collections::HashMap;` at the top of the file.

- [ ] **Step 2: Update lib.rs to use new function**

In `crates/vela-core/src/lib.rs`, update the `optimize` function:

```rust
pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    component::process_component(wasm, |module_bytes| {
        if config.dce {
            dce::eliminate_dead_functions(module_bytes)
        } else {
            Ok(module_bytes.to_vec())
        }
    })
}
```

- [ ] **Step 3: Update existing tests**

In `crates/vela-core/src/dce.rs`, update the `eliminates_dead_function_body` test:

```rust
    #[test]
    fn eliminates_dead_functions() {
        let wasm = build_basic_module();
        let optimized = eliminate_dead_functions(&wasm).expect("should optimize");

        // Validate the optimized module
        wasmparser::Validator::new().validate_all(&optimized).expect("optimized module should be valid");

        // Dead func 3 should be fully removed — only 3 functions remain
        let graph = CallGraph::from_module(&optimized).expect("should parse optimized module");
        assert_eq!(graph.num_functions, 3, "dead function should be fully removed");

        // Should be smaller
        assert!(optimized.len() < wasm.len(), "optimized should be smaller");
    }
```

Update the `live_functions_preserved_correctly` test to use `eliminate_dead_functions`:

```rust
    #[test]
    fn live_functions_preserved_correctly() {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("get_42", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::I32Const(42));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(99));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        module.section(&codes);
        let wasm = module.finish();

        let optimized = eliminate_dead_functions(&wasm).expect("should optimize");

        wasmparser::Validator::new().validate_all(&optimized).expect("should be valid");

        // Dead func 1 removed, only 1 function remains
        let graph = CallGraph::from_module(&optimized).unwrap();
        assert_eq!(graph.num_functions, 1, "only exported function should remain");

        // Verify func 0 still contains i32.const 42
        let parser = wasmparser::Parser::new(0);
        let mut found_42 = false;
        for payload in parser.parse_all(&optimized) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut ops = body.get_operators_reader().unwrap();
                while !ops.eof() {
                    if let wasmparser::Operator::I32Const { value: 42 } = ops.read().unwrap() {
                        found_42 = true;
                    }
                }
            }
        }
        assert!(found_42, "func 0 should still contain i32.const 42 after DCE");
    }
```

- [ ] **Step 4: Remove unused imports**

In `dce.rs`, remove the `RoundtripReencoder` import since `eliminate_dead_code` no longer does its own re-encoding:

```rust
// Remove this line:
// use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p vela-core`
Expected: All tests pass (existing + updated).

- [ ] **Step 6: Commit**

```bash
git add crates/vela-core/src/dce.rs crates/vela-core/src/lib.rs
git commit -m "feat: DCE now fully removes dead functions with index renumbering"
```

---

### Task 4: DFE — Duplicate Function Elimination

**Files:**
- Create: `crates/vela-core/src/dfe.rs`
- Modify: `crates/vela-core/src/lib.rs`
- Modify: `crates/vela-core/src/testutil.rs`

- [ ] **Step 1: Add test helper to testutil.rs**

Add to `crates/vela-core/src/testutil.rs`:

```rust
/// Build a core module with duplicate functions:
/// - func 0: exported "entry", calls func 1 and func 3
/// - func 1: () -> i32, returns 42
/// - func 2: dead leaf
/// - func 3: () -> i32, returns 42 (duplicate of func 1)
pub fn build_module_with_duplicates() -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![]);         // type 0: () -> ()
    types.ty().function(vec![], vec![ValType::I32]); // type 1: () -> i32
    module.section(&types);

    let mut functions = FunctionSection::new();
    functions.function(0); // func 0: () -> ()
    functions.function(1); // func 1: () -> i32
    functions.function(0); // func 2: () -> ()  (dead)
    functions.function(1); // func 3: () -> i32 (duplicate of func 1)
    module.section(&functions);

    let mut exports = ExportSection::new();
    exports.export("entry", ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = CodeSection::new();

    // func 0: calls func 1 and func 3
    let mut f0 = Function::new(vec![]);
    f0.instruction(&Instruction::Call(1));
    f0.instruction(&Instruction::Drop);
    f0.instruction(&Instruction::Call(3));
    f0.instruction(&Instruction::Drop);
    f0.instruction(&Instruction::End);
    codes.function(&f0);

    // func 1: returns 42
    let mut f1 = Function::new(vec![]);
    f1.instruction(&Instruction::I32Const(42));
    f1.instruction(&Instruction::End);
    codes.function(&f1);

    // func 2: dead leaf
    let mut f2 = Function::new(vec![]);
    f2.instruction(&Instruction::End);
    codes.function(&f2);

    // func 3: returns 42 (same type and body as func 1)
    let mut f3 = Function::new(vec![]);
    f3.instruction(&Instruction::I32Const(42));
    f3.instruction(&Instruction::End);
    codes.function(&f3);

    module.section(&codes);
    module.finish()
}
```

- [ ] **Step 2: Create `dfe.rs` with test and `todo!()`**

Create `crates/vela-core/src/dfe.rs`:

```rust
// crates/vela-core/src/dfe.rs
use std::collections::{HashMap, HashSet};
use crate::error::VelaError;

pub struct DfeResult {
    pub redirects: HashMap<u32, u32>,
    pub removals: HashSet<u32>,
}

/// Detect duplicate functions among reachable defined functions.
/// Two functions are duplicates if they have the same type_index and identical body bytes.
pub fn find_duplicates(
    module_bytes: &[u8],
    reachable: &HashSet<u32>,
) -> Result<DfeResult, VelaError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::build_module_with_duplicates;
    use crate::callgraph::CallGraph;
    use crate::dce::{find_roots, find_reachable};

    #[test]
    fn detects_duplicate_functions() {
        let wasm = build_module_with_duplicates();
        let graph = CallGraph::from_module(&wasm).unwrap();
        let roots = find_roots(&wasm, &graph).unwrap();
        let reachable = find_reachable(&roots, &graph);

        let result = find_duplicates(&wasm, &reachable).unwrap();

        // func 3 is a duplicate of func 1 (same type, same body)
        assert_eq!(result.redirects.len(), 1);
        assert_eq!(result.redirects[&3], 1);
        assert!(result.removals.contains(&3));

        // func 2 is dead so not considered for DFE
        assert!(!result.redirects.contains_key(&2));
    }

    #[test]
    fn no_duplicates_when_types_differ() {
        use wasm_encoder::*;

        // func 0: exported, () -> ()
        // func 1: () -> i32, returns 42
        // func 2: () -> (), body is just End (different type from func 1, even if body similar)
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(1);
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("entry", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(1));
        f0.instruction(&Instruction::Drop);
        f0.instruction(&Instruction::Call(2));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(42));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(42));
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        module.section(&codes);
        let wasm = module.finish();

        let graph = CallGraph::from_module(&wasm).unwrap();
        let roots = find_roots(&wasm, &graph).unwrap();
        let reachable = find_reachable(&roots, &graph);

        let result = find_duplicates(&wasm, &reachable).unwrap();
        assert!(result.redirects.is_empty(), "different types should not be duplicates");
    }
}
```

- [ ] **Step 3: Add dfe module to lib.rs**

Add to `crates/vela-core/src/lib.rs` after existing modules:

```rust
pub mod dfe;
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p vela-core detects_duplicate`
Expected: FAIL with `not yet implemented`

- [ ] **Step 5: Implement `find_duplicates`**

Replace the `todo!()`:

```rust
pub fn find_duplicates(
    module_bytes: &[u8],
    reachable: &HashSet<u32>,
) -> Result<DfeResult, VelaError> {
    use std::hash::{Hash, Hasher};

    let parser = wasmparser::Parser::new(0);
    let mut num_imports: u32 = 0;
    let mut type_indices: Vec<u32> = Vec::new();
    let mut body_bytes: Vec<Vec<u8>> = Vec::new();

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        num_imports += 1;
                    }
                }
            }
            wasmparser::Payload::FunctionSection(reader) => {
                for type_idx in reader {
                    type_indices.push(type_idx?);
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let range = body.range();
                body_bytes.push(module_bytes[range.start..range.end].to_vec());
            }
            _ => {}
        }
    }

    // Group reachable defined functions by (type_index, body_hash)
    // key → (representative func_index, representative body bytes)
    let mut groups: HashMap<(u32, u64), (u32, Vec<u8>)> = HashMap::new();
    let mut redirects = HashMap::new();
    let mut removals = HashSet::new();

    for (code_idx, bytes) in body_bytes.iter().enumerate() {
        let func_idx = num_imports + code_idx as u32;
        if !reachable.contains(&func_idx) {
            continue;
        }

        let type_idx = type_indices[code_idx];
        let mut hasher = std::hash::DefaultHasher::new();
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        let key = (type_idx, hash);

        match groups.get(&key) {
            Some((representative, rep_bytes)) if rep_bytes == bytes => {
                redirects.insert(func_idx, *representative);
                removals.insert(func_idx);
            }
            Some(_) => {
                // Hash collision but different body — treat as unique
            }
            None => {
                groups.insert(key, (func_idx, bytes.clone()));
            }
        }
    }

    Ok(DfeResult { redirects, removals })
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p vela-core dfe`
Expected: Both tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/vela-core/src/dfe.rs crates/vela-core/src/lib.rs crates/vela-core/src/testutil.rs
git commit -m "feat: DFE duplicate function detection"
```

---

### Task 5: Integrated Pipeline — `optimize_module`

**Files:**
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Update `OptimizeConfig` and `optimize`**

Replace the contents of `crates/vela-core/src/lib.rs` with:

```rust
// crates/vela-core/src/lib.rs
pub mod callgraph;
pub mod component;
pub mod dce;
pub mod dfe;
pub mod error;
pub mod renumber;
#[cfg(test)]
pub(crate) mod testutil;

pub use error::VelaError;

use std::collections::{HashMap, HashSet};

/// Configuration for optimization passes.
pub struct OptimizeConfig {
    /// Enable Dead Code Elimination.
    pub dce: bool,
    /// Enable Duplicate Function Elimination.
    pub dfe: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { dce: true, dfe: true }
    }
}

/// Optimize a Component Model WASM binary.
pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    component::process_component(wasm, |module_bytes| {
        optimize_module(module_bytes, config)
    })
}

fn optimize_module(module_bytes: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    let graph = callgraph::CallGraph::from_module(module_bytes)?;
    let roots = dce::find_roots(module_bytes, &graph)?;
    let reachable = dce::find_reachable(&roots, &graph);

    let mut removals: HashSet<u32> = if config.dce {
        (0..graph.num_functions)
            .filter(|i| !reachable.contains(i))
            .collect()
    } else {
        HashSet::new()
    };

    let mut redirects: HashMap<u32, u32> = HashMap::new();
    if config.dfe {
        let dfe_result = dfe::find_duplicates(module_bytes, &reachable)?;
        redirects = dfe_result.redirects;
        removals.extend(dfe_result.removals);
    }

    if removals.is_empty() && redirects.is_empty() {
        return Ok(module_bytes.to_vec());
    }

    let index_map = renumber::build_index_map(graph.num_functions, &redirects, &removals);
    renumber::rebuild_module(module_bytes, &index_map, &removals, graph.num_imports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::{
        CodeSection, Component, ExportKind, ExportSection, Function, FunctionSection, Instruction,
        Module, TypeSection,
    };

    fn build_component_with_dead_code() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![wasm_encoder::ValType::I32]);
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(1);
        functions.function(1);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("answer", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::I32Const(42));
        f0.instruction(&Instruction::End);
        code.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(1));
        f1.instruction(&Instruction::I32Const(2));
        f1.instruction(&Instruction::I32Add);
        f1.instruction(&Instruction::Drop);
        f1.instruction(&Instruction::End);
        code.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(100));
        f2.instruction(&Instruction::I32Const(200));
        f2.instruction(&Instruction::I32Mul);
        f2.instruction(&Instruction::Drop);
        f2.instruction(&Instruction::End);
        code.function(&f2);

        module.section(&code);

        let mut component = Component::new();
        component.section(&wasm_encoder::ModuleSection(&module));
        component.finish()
    }

    #[test]
    fn optimize_reduces_component_size() {
        let original = build_component_with_dead_code();
        let config = OptimizeConfig { dce: true, dfe: true };
        let optimized = optimize(&original, &config).expect("optimize should succeed");

        assert!(
            optimized.len() < original.len(),
            "optimized ({}) should be smaller than original ({})",
            optimized.len(),
            original.len()
        );

        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&optimized) {
            payload.expect("optimized should be parseable");
        }
    }

    #[test]
    fn optimize_with_all_disabled_passes_through() {
        let original = build_component_with_dead_code();
        let config = OptimizeConfig { dce: false, dfe: false };
        let result = optimize(&original, &config).expect("should succeed");

        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&result) {
            payload.expect("result should be parseable");
        }
    }

    #[test]
    fn optimize_rejects_core_module() {
        let module = Module::new().finish();
        let result = optimize(&module, &OptimizeConfig::default());
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p vela-core`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/vela-core/src/lib.rs
git commit -m "feat: integrated optimize_module pipeline with DCE + DFE"
```

---

### Task 6: CLI — Add `--no-dfe` Flag

**Files:**
- Modify: `crates/vela-cli/src/main.rs`

- [ ] **Step 1: Update CLI**

In `crates/vela-cli/src/main.rs`, add the `no_dfe` field to the `Optimize` variant:

```rust
    Optimize {
        /// Input .wasm file
        input: PathBuf,

        /// Output .wasm file
        #[arg(short, long)]
        output: PathBuf,

        /// Disable Dead Code Elimination
        #[arg(long)]
        no_dce: bool,

        /// Disable Duplicate Function Elimination
        #[arg(long)]
        no_dfe: bool,
    },
```

Update the match arm to include `no_dfe`:

```rust
        Commands::Optimize { input, output, no_dce, no_dfe } => {
            let wasm = std::fs::read(&input)?;
            let input_size = wasm.len();

            let config = vela_core::OptimizeConfig { dce: !no_dce, dfe: !no_dfe };
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: Compiles.

Run: `cargo run -- optimize --help`
Expected: Shows `--no-dce` and `--no-dfe` flags.

- [ ] **Step 3: Commit**

```bash
git add crates/vela-cli/src/main.rs
git commit -m "feat: add --no-dfe CLI flag"
```

---

### Task 7: Integration Test — DCE + DFE with wasmtime

**Files:**
- Modify: `crates/vela-core/tests/integration_test.rs`

- [ ] **Step 1: Update existing tests for new OptimizeConfig**

Update existing tests to include `dfe` field:

```rust
#[test]
fn optimized_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let optimized = optimize(&original, &OptimizeConfig { dce: true, dfe: true }).expect("optimize should succeed");

    assert!(
        optimized.len() < original.len(),
        "optimized ({}) should be smaller than original ({})",
        optimized.len(),
        original.len()
    );

    assert_eq!(call_answer(&optimized), 42);
}

#[test]
fn pass_through_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let result = optimize(&original, &OptimizeConfig { dce: false, dfe: false }).expect("pass-through should succeed");
    assert_eq!(call_answer(&result), 42);
}
```

- [ ] **Step 2: Add DFE integration test**

Add a new test with duplicate functions:

```rust
fn build_component_with_duplicates_wat() -> Vec<u8> {
    wat::parse_str(
        r#"
        (component
            (core module $m
                (func $get_a (export "get_a") (result i32)
                    i32.const 42
                )
                (func $get_b (export "get_b") (result i32)
                    i32.const 42
                )
                (func $dead (result i32)
                    i32.const 99
                )
            )
            (core instance $i (instantiate $m))
            (func (export "get_a") (result u32)
                (canon lift (core func $i "get_a"))
            )
            (func (export "get_b") (result u32)
                (canon lift (core func $i "get_b"))
            )
        )
    "#,
    )
    .expect("WAT should parse")
}

fn call_func(wasm: &[u8], name: &str) -> u32 {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&config).expect("engine");
    let mut store = wasmtime::Store::new(&engine, ());

    let component = wasmtime::component::Component::new(&engine, wasm).expect("should compile");
    let linker: wasmtime::component::Linker<()> = wasmtime::component::Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &component)
        .expect("should instantiate");

    let func = instance
        .get_typed_func::<(), (u32,)>(&mut store, name)
        .expect("should find export");
    let (result,) = func.call(&mut store, ()).expect("should call");
    result
}

#[test]
fn dfe_merges_duplicates_and_runs_correctly() {
    let original = build_component_with_duplicates_wat();
    let optimized = optimize(&original, &OptimizeConfig { dce: true, dfe: true })
        .expect("optimize should succeed");

    assert!(optimized.len() < original.len());

    // Both exports should still work correctly
    assert_eq!(call_func(&optimized, "get_a"), 42);
    assert_eq!(call_func(&optimized, "get_b"), 42);
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test -p vela-core --test integration_test`
Expected: All tests pass (existing + new DFE test).

- [ ] **Step 4: Commit**

```bash
git add crates/vela-core/tests/integration_test.rs
git commit -m "test: integration tests for DCE + DFE with wasmtime"
```

---

## Summary

| Task | Description | Key Output |
|------|-------------|------------|
| 1 | `build_index_map` | Index mapping with redirects + removals |
| 2 | `FunctionRenumberer` + `rebuild_module` | Module re-encoding with function deletion |
| 3 | DCE improvement | `eliminate_dead_functions` replaces body substitution |
| 4 | DFE | `find_duplicates` with `(type_index, body_hash)` key |
| 5 | Integrated pipeline | `optimize_module` combining DCE + DFE |
| 6 | CLI update | `--no-dfe` flag |
| 7 | Integration tests | wasmtime verification of DFE correctness |
