# Vela Phase 3: RUME — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove unused tables, memories, globals, and imports from WASM core modules, extending the Phase 2 index renumbering infrastructure to all index spaces.

**Architecture:** New `analyze_usage` scans reachable function bodies and sections for table/memory/global references. `FunctionRenumberer` is extended to `ModuleRenumberer` with maps for all index spaces. `rebuild_module` is extended to skip removed imports, tables, memories, and globals. The `optimize_module` pipeline gains a RUME pass after DCE+DFE.

**Tech Stack:** Rust 1.93+, wasmparser 0.246, wasm-encoder 0.246 (reencode feature), clap 4, wasmtime 43 (test only)

---

## File Map

| File | Responsibility |
|------|---------------|
| `crates/vela-core/src/rume.rs` | New: `analyze_usage`, `UsageInfo`, `ModuleCounts` |
| `crates/vela-core/src/renumber.rs` | `FunctionRenumberer` → `ModuleRenumberer`; `rebuild_module` extended for all index spaces |
| `crates/vela-core/src/lib.rs` | Add `rume` to `OptimizeConfig`; RUME in `optimize_module` pipeline |
| `crates/vela-cli/src/main.rs` | Add `--no-rume` flag |
| `crates/vela-core/tests/integration_test.rs` | Add RUME wasmtime integration test |

---

### Task 1: Usage Analysis — `analyze_usage`

**Files:**
- Create: `crates/vela-core/src/rume.rs`
- Modify: `crates/vela-core/src/lib.rs` (add `pub mod rume;`)

- [ ] **Step 1: Create rume.rs with UsageInfo, ModuleCounts, and todo!()**

```rust
// crates/vela-core/src/rume.rs
use std::collections::HashSet;
use crate::error::VelaError;

/// Counts of each element kind in the module (imports + defined).
pub struct ModuleCounts {
    pub num_functions: u32,
    pub num_tables: u32,
    pub num_memories: u32,
    pub num_globals: u32,
    // Import counts (subset of the above)
    pub num_func_imports: u32,
    pub num_table_imports: u32,
    pub num_memory_imports: u32,
    pub num_global_imports: u32,
}

/// Which tables, memories, and globals are actually used.
pub struct UsageInfo {
    pub used_tables: HashSet<u32>,
    pub used_memories: HashSet<u32>,
    pub used_globals: HashSet<u32>,
    pub counts: ModuleCounts,
}

/// Analyze which tables, memories, and globals are used in the module.
/// Only scans reachable function bodies (dead code is skipped).
pub fn analyze_usage(
    module_bytes: &[u8],
    reachable: &HashSet<u32>,
) -> Result<UsageInfo, VelaError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::*;

    #[test]
    fn detects_used_global() {
        // func 0: exported, uses global 0
        // global 0: i32, mutable
        // global 1: i32, mutable (unused)
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut globals = GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(0),
        );
        globals.global(
            wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(0),
        );
        module.section(&globals);

        let mut exports = ExportSection::new();
        exports.export("get", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::GlobalGet(0));
        f0.instruction(&Instruction::End);
        codes.function(&f0);
        module.section(&codes);

        let wasm = module.finish();

        let reachable = HashSet::from([0]);
        let info = analyze_usage(&wasm, &reachable).unwrap();

        assert!(info.used_globals.contains(&0), "global 0 should be used");
        assert!(!info.used_globals.contains(&1), "global 1 should be unused");
    }

    #[test]
    fn exported_elements_are_used() {
        // global 0: exported → used
        // memory 0: exported → used
        // table 0: not exported, not referenced → unused
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut tables = TableSection::new();
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            minimum: 1,
            maximum: None,
            table64: false,
            shared: false,
        });
        module.section(&tables);

        let mut memories = MemorySection::new();
        memories.memory(MemoryType { minimum: 1, maximum: None, memory64: false, shared: false, page_size_log2: None });
        module.section(&memories);

        let mut globals = GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType { val_type: ValType::I32, mutable: false, shared: false },
            &ConstExpr::i32_const(42),
        );
        module.section(&globals);

        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, 0);
        exports.export("g", ExportKind::Global, 0);
        exports.export("mem", ExportKind::Memory, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::End);
        codes.function(&f0);
        module.section(&codes);

        let wasm = module.finish();

        let reachable = HashSet::from([0]);
        let info = analyze_usage(&wasm, &reachable).unwrap();

        assert!(info.used_globals.contains(&0), "exported global should be used");
        assert!(info.used_memories.contains(&0), "exported memory should be used");
        assert!(!info.used_tables.contains(&0), "unexported/unreferenced table should be unused");
    }

    #[test]
    fn dead_function_references_ignored() {
        // func 0: exported, no global use
        // func 1: dead, uses global 0
        // global 0: should be unused (only referenced from dead code)
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(1);
        module.section(&functions);

        let mut globals = GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(0),
        );
        module.section(&globals);

        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::GlobalGet(0));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        module.section(&codes);
        let wasm = module.finish();

        // Only func 0 is reachable
        let reachable = HashSet::from([0]);
        let info = analyze_usage(&wasm, &reachable).unwrap();

        assert!(!info.used_globals.contains(&0), "global only used by dead func should be unused");
    }
}
```

- [ ] **Step 2: Add `pub mod rume;` to lib.rs**

Add after existing module declarations.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p vela-core rume`
Expected: FAIL with `not yet implemented`

- [ ] **Step 4: Implement `analyze_usage`**

Replace the `todo!()`:

```rust
pub fn analyze_usage(
    module_bytes: &[u8],
    reachable: &HashSet<u32>,
) -> Result<UsageInfo, VelaError> {
    let parser = wasmparser::Parser::new(0);
    let mut used_tables = HashSet::new();
    let mut used_memories = HashSet::new();
    let mut used_globals = HashSet::new();

    let mut num_func_imports: u32 = 0;
    let mut num_table_imports: u32 = 0;
    let mut num_memory_imports: u32 = 0;
    let mut num_global_imports: u32 = 0;
    let mut num_defined_tables: u32 = 0;
    let mut num_defined_memories: u32 = 0;
    let mut num_defined_globals: u32 = 0;
    let mut num_defined_functions: u32 = 0;
    let mut code_index: u32 = 0;

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    match import.ty {
                        wasmparser::TypeRef::Func(_) => num_func_imports += 1,
                        wasmparser::TypeRef::Table(_) => num_table_imports += 1,
                        wasmparser::TypeRef::Memory(_) => num_memory_imports += 1,
                        wasmparser::TypeRef::Global(_) => num_global_imports += 1,
                        _ => {}
                    }
                }
            }
            wasmparser::Payload::FunctionSection(reader) => {
                num_defined_functions = reader.count();
            }
            wasmparser::Payload::TableSection(reader) => {
                num_defined_tables = reader.count();
            }
            wasmparser::Payload::MemorySection(reader) => {
                num_defined_memories = reader.count();
            }
            wasmparser::Payload::GlobalSection(reader) => {
                for global in reader {
                    let global = global?;
                    // Check init expr for global.get references
                    let mut init_reader = global.init_expr.get_operators_reader();
                    while !init_reader.eof() {
                        if let wasmparser::Operator::GlobalGet { global_index } = init_reader.read()? {
                            used_globals.insert(global_index);
                        }
                    }
                    num_defined_globals += 1;
                }
            }
            wasmparser::Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    match export.kind {
                        wasmparser::ExternalKind::Table => { used_tables.insert(export.index); }
                        wasmparser::ExternalKind::Memory => { used_memories.insert(export.index); }
                        wasmparser::ExternalKind::Global => { used_globals.insert(export.index); }
                        _ => {}
                    }
                }
            }
            wasmparser::Payload::ElementSection(reader) => {
                for elem in reader {
                    let elem = elem?;
                    if let wasmparser::ElementKind::Active { table_index, .. } = elem.kind {
                        used_tables.insert(table_index.unwrap_or(0));
                    }
                }
            }
            wasmparser::Payload::DataSection(reader) => {
                for data in reader {
                    let data = data?;
                    if let wasmparser::DataKind::Active { memory_index, .. } = data.kind {
                        used_memories.insert(memory_index);
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let func_index = num_func_imports + code_index;
                code_index += 1;

                if !reachable.contains(&func_index) {
                    continue;
                }

                let mut ops = body.get_operators_reader()?;
                while !ops.eof() {
                    match ops.read()? {
                        wasmparser::Operator::GlobalGet { global_index }
                        | wasmparser::Operator::GlobalSet { global_index } => {
                            used_globals.insert(global_index);
                        }
                        wasmparser::Operator::TableGet { table }
                        | wasmparser::Operator::TableSet { table }
                        | wasmparser::Operator::TableGrow { table }
                        | wasmparser::Operator::TableSize { table }
                        | wasmparser::Operator::TableFill { table } => {
                            used_tables.insert(table);
                        }
                        wasmparser::Operator::TableCopy { dst_table, src_table } => {
                            used_tables.insert(dst_table);
                            used_tables.insert(src_table);
                        }
                        wasmparser::Operator::TableInit { table, .. } => {
                            used_tables.insert(table);
                        }
                        wasmparser::Operator::CallIndirect { table_index, .. }
                        | wasmparser::Operator::ReturnCallIndirect { table_index, .. } => {
                            used_tables.insert(table_index);
                        }
                        wasmparser::Operator::MemorySize { mem }
                        | wasmparser::Operator::MemoryGrow { mem }
                        | wasmparser::Operator::MemoryFill { mem } => {
                            used_memories.insert(mem);
                        }
                        wasmparser::Operator::MemoryCopy { dst_mem, src_mem } => {
                            used_memories.insert(dst_mem);
                            used_memories.insert(src_mem);
                        }
                        wasmparser::Operator::MemoryInit { mem, .. } => {
                            used_memories.insert(mem);
                        }
                        wasmparser::Operator::I32Load { memarg }
                        | wasmparser::Operator::I64Load { memarg }
                        | wasmparser::Operator::F32Load { memarg }
                        | wasmparser::Operator::F64Load { memarg }
                        | wasmparser::Operator::I32Load8S { memarg }
                        | wasmparser::Operator::I32Load8U { memarg }
                        | wasmparser::Operator::I32Load16S { memarg }
                        | wasmparser::Operator::I32Load16U { memarg }
                        | wasmparser::Operator::I64Load8S { memarg }
                        | wasmparser::Operator::I64Load8U { memarg }
                        | wasmparser::Operator::I64Load16S { memarg }
                        | wasmparser::Operator::I64Load16U { memarg }
                        | wasmparser::Operator::I64Load32S { memarg }
                        | wasmparser::Operator::I64Load32U { memarg }
                        | wasmparser::Operator::I32Store { memarg }
                        | wasmparser::Operator::I64Store { memarg }
                        | wasmparser::Operator::F32Store { memarg }
                        | wasmparser::Operator::F64Store { memarg }
                        | wasmparser::Operator::I32Store8 { memarg }
                        | wasmparser::Operator::I32Store16 { memarg }
                        | wasmparser::Operator::I64Store8 { memarg }
                        | wasmparser::Operator::I64Store16 { memarg }
                        | wasmparser::Operator::I64Store32 { memarg } => {
                            used_memories.insert(memarg.memory);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(UsageInfo {
        used_tables,
        used_memories,
        used_globals,
        counts: ModuleCounts {
            num_functions: num_func_imports + num_defined_functions,
            num_tables: num_table_imports + num_defined_tables,
            num_memories: num_memory_imports + num_defined_memories,
            num_globals: num_global_imports + num_defined_globals,
            num_func_imports,
            num_table_imports,
            num_memory_imports,
            num_global_imports,
        },
    })
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p vela-core rume`
Expected: All 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/vela-core/src/rume.rs crates/vela-core/src/lib.rs
git commit -m "feat: RUME usage analysis for tables, memories, and globals"
```

---

### Task 2: ModuleRenumberer — Extend to All Index Spaces

**Files:**
- Modify: `crates/vela-core/src/renumber.rs`

- [ ] **Step 1: Rename `FunctionRenumberer` to `ModuleRenumberer` and add index maps**

Replace the `FunctionRenumberer` struct and impl with:

```rust
/// A re-encoder that remaps function, table, memory, and global indices
/// according to precomputed maps.
pub struct ModuleRenumberer {
    pub function_map: Vec<u32>,
    pub table_map: Vec<u32>,
    pub memory_map: Vec<u32>,
    pub global_map: Vec<u32>,
}

impl ModuleRenumberer {
    /// Create a renumberer that only remaps functions (backward-compatible).
    pub fn function_only(function_map: Vec<u32>) -> Self {
        Self {
            function_map,
            table_map: Vec::new(),
            memory_map: Vec::new(),
            global_map: Vec::new(),
        }
    }
}

impl Reencode for ModuleRenumberer {
    type Error = Infallible;

    fn function_index(&mut self, func: u32) -> Result<u32, Error<Self::Error>> {
        Ok(self.function_map[func as usize])
    }

    fn table_index(&mut self, table: u32) -> Result<u32, Error<Self::Error>> {
        if self.table_map.is_empty() {
            Ok(table)
        } else {
            Ok(self.table_map[table as usize])
        }
    }

    fn memory_index(&mut self, memory: u32) -> Result<u32, Error<Self::Error>> {
        if self.memory_map.is_empty() {
            Ok(memory)
        } else {
            Ok(self.memory_map[memory as usize])
        }
    }

    fn global_index(&mut self, global: u32) -> Result<u32, Error<Self::Error>> {
        if self.global_map.is_empty() {
            Ok(global)
        } else {
            Ok(self.global_map[global as usize])
        }
    }
}
```

- [ ] **Step 2: Add `Removals` struct and update `rebuild_module` signature**

Add a struct to hold all removal sets:

```rust
/// Sets of indices to remove across all index spaces.
pub struct Removals {
    pub functions: HashSet<u32>,
    pub tables: HashSet<u32>,
    pub memories: HashSet<u32>,
    pub globals: HashSet<u32>,
}

impl Removals {
    pub fn functions_only(functions: HashSet<u32>) -> Self {
        Self {
            functions,
            tables: HashSet::new(),
            memories: HashSet::new(),
            globals: HashSet::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.tables.is_empty()
            && self.memories.is_empty()
            && self.globals.is_empty()
    }
}
```

- [ ] **Step 3: Update `rebuild_module` to accept `ModuleRenumberer` and `Removals`**

Change the signature and update the body. Key changes:

```rust
pub fn rebuild_module(
    module_bytes: &[u8],
    reencoder: &mut ModuleRenumberer,
    removals: &Removals,
    num_imports: &crate::rume::ModuleCounts,
) -> Result<Vec<u8>, VelaError>
```

The import section handler needs to track per-import-kind indices and skip removed ones:

```rust
wasmparser::Payload::ImportSection(reader) => {
    let mut sec = wasm_encoder::ImportSection::new();
    let mut func_idx: u32 = 0;
    let mut table_idx: u32 = 0;
    let mut memory_idx: u32 = 0;
    let mut global_idx: u32 = 0;

    for import in reader.into_imports() {
        let import = import?;
        let should_skip = match import.ty {
            wasmparser::TypeRef::Func(_) => {
                let skip = removals.functions.contains(&func_idx);
                func_idx += 1;
                skip
            }
            wasmparser::TypeRef::Table(_) => {
                let skip = removals.tables.contains(&table_idx);
                table_idx += 1;
                skip
            }
            wasmparser::TypeRef::Memory(_) => {
                let skip = removals.memories.contains(&memory_idx);
                memory_idx += 1;
                skip
            }
            wasmparser::TypeRef::Global(_) => {
                let skip = removals.globals.contains(&global_idx);
                global_idx += 1;
                skip
            }
            _ => false,
        };
        if !should_skip {
            reencoder.parse_import(&mut sec, import).map_err(&enc_err)?;
        }
    }
    module.section(&sec);
}
```

Table section, memory section, and global section also skip removed entries:

```rust
wasmparser::Payload::TableSection(reader) => {
    let mut sec = wasm_encoder::TableSection::new();
    for (i, table) in reader.into_iter().enumerate() {
        let idx = num_imports.num_table_imports + i as u32;
        if !removals.tables.contains(&idx) {
            reencoder.parse_table(&mut sec, table?).map_err(&enc_err)?;
        }
    }
    module.section(&sec);
}
```

(Same pattern for memory and global sections.)

Function section and code section use `removals.functions` (same as before but accessing via struct field).

- [ ] **Step 4: Update existing tests**

All existing `rebuild_module` call sites need to be updated to use the new types. Update the two existing tests in `renumber.rs`:

For `rebuild_removes_dead_function`:
```rust
let removals = Removals::functions_only(HashSet::from([2u32]));
let index_map = build_index_map(3, &HashMap::new(), &removals.functions);
let mut reencoder = ModuleRenumberer::function_only(index_map);
let counts = crate::rume::ModuleCounts {
    num_functions: 3, num_tables: 0, num_memories: 0, num_globals: 0,
    num_func_imports: 0, num_table_imports: 0, num_memory_imports: 0, num_global_imports: 0,
};
let rebuilt = rebuild_module(&wasm, &mut reencoder, &removals, &counts).expect("should succeed");
```

Similarly for `rebuild_with_redirect_and_removal`.

- [ ] **Step 5: Add test for removing unused global**

```rust
    #[test]
    fn rebuild_removes_unused_global() {
        use wasm_encoder::*;

        // func 0: exported, uses global 0
        // global 0: used
        // global 1: unused
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut globals = GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(42),
        );
        globals.global(
            wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(99),
        );
        module.section(&globals);

        let mut exports = ExportSection::new();
        exports.export("get", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::GlobalGet(0));
        f0.instruction(&Instruction::End);
        codes.function(&f0);
        module.section(&codes);

        let wasm = module.finish();

        let removals = Removals {
            functions: HashSet::new(),
            tables: HashSet::new(),
            memories: HashSet::new(),
            globals: HashSet::from([1]),
        };
        let func_map = build_index_map(1, &HashMap::new(), &removals.functions);
        let global_map = build_index_map(2, &HashMap::new(), &removals.globals);
        let mut reencoder = ModuleRenumberer {
            function_map: func_map,
            table_map: Vec::new(),
            memory_map: Vec::new(),
            global_map,
        };
        let counts = crate::rume::ModuleCounts {
            num_functions: 1, num_tables: 0, num_memories: 0, num_globals: 2,
            num_func_imports: 0, num_table_imports: 0, num_memory_imports: 0, num_global_imports: 0,
        };
        let rebuilt = rebuild_module(&wasm, &mut reencoder, &removals, &counts).expect("should succeed");

        wasmparser::Validator::new().validate_all(&rebuilt).expect("should be valid");
        assert!(rebuilt.len() < wasm.len(), "should be smaller after removing unused global");
    }
```

- [ ] **Step 6: Run all renumber tests**

Run: `cargo test -p vela-core renumber`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/vela-core/src/renumber.rs
git commit -m "feat: ModuleRenumberer with all index spaces and extended rebuild_module"
```

---

### Task 3: Integrate RUME into Pipeline + Update Callers

**Files:**
- Modify: `crates/vela-core/src/lib.rs`
- Modify: `crates/vela-core/src/dce.rs` (update `eliminate_dead_functions` to use new types)

- [ ] **Step 1: Update `OptimizeConfig` and `optimize_module`**

Update `crates/vela-core/src/lib.rs`:

```rust
pub struct OptimizeConfig {
    pub dce: bool,
    pub dfe: bool,
    pub rume: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { dce: true, dfe: true, rume: true }
    }
}
```

Update `optimize_module` to include RUME:

```rust
fn optimize_module(module_bytes: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    if !config.dce && !config.dfe && !config.rume {
        return Ok(module_bytes.to_vec());
    }

    let graph = callgraph::CallGraph::from_module(module_bytes)?;
    let roots = dce::find_roots(module_bytes, &graph)?;
    let reachable = dce::find_reachable(&roots, &graph);

    // DCE
    let mut func_removals: HashSet<u32> = if config.dce {
        (0..graph.num_functions)
            .filter(|i| !reachable.contains(i))
            .collect()
    } else {
        HashSet::new()
    };

    // DFE
    let mut redirects: HashMap<u32, u32> = HashMap::new();
    if config.dfe {
        let dfe_result = dfe::find_duplicates(module_bytes, &reachable)?;
        redirects = dfe_result.redirects;
        func_removals.extend(dfe_result.removals);
    }

    // RUME
    let usage = rume::analyze_usage(module_bytes, &reachable)?;
    let counts = &usage.counts;

    let mut table_removals = HashSet::new();
    let mut memory_removals = HashSet::new();
    let mut global_removals = HashSet::new();

    if config.rume {
        for i in 0..counts.num_tables {
            if !usage.used_tables.contains(&i) {
                table_removals.insert(i);
            }
        }
        for i in 0..counts.num_memories {
            if !usage.used_memories.contains(&i) {
                memory_removals.insert(i);
            }
        }
        for i in 0..counts.num_globals {
            if !usage.used_globals.contains(&i) {
                global_removals.insert(i);
            }
        }
    }

    let removals = renumber::Removals {
        functions: func_removals,
        tables: table_removals,
        memories: memory_removals,
        globals: global_removals,
    };

    if removals.is_empty() && redirects.is_empty() {
        return Ok(module_bytes.to_vec());
    }

    let func_map = renumber::build_index_map(counts.num_functions, &redirects, &removals.functions);
    let table_map = renumber::build_index_map(counts.num_tables, &HashMap::new(), &removals.tables);
    let memory_map = renumber::build_index_map(counts.num_memories, &HashMap::new(), &removals.memories);
    let global_map = renumber::build_index_map(counts.num_globals, &HashMap::new(), &removals.globals);

    let mut reencoder = renumber::ModuleRenumberer {
        function_map: func_map,
        table_map,
        memory_map,
        global_map,
    };

    renumber::rebuild_module(module_bytes, &mut reencoder, &removals, counts)
}
```

- [ ] **Step 2: Update `dce::eliminate_dead_functions`**

Update to use the new `ModuleRenumberer` and `Removals` types:

```rust
pub fn eliminate_dead_functions(module_bytes: &[u8]) -> Result<Vec<u8>, VelaError> {
    let graph = CallGraph::from_module(module_bytes)?;
    let roots = find_roots(module_bytes, &graph)?;
    let reachable = find_reachable(&roots, &graph);

    let func_removals: HashSet<u32> = (0..graph.num_functions)
        .filter(|i| !reachable.contains(i))
        .collect();

    if func_removals.is_empty() {
        return Ok(module_bytes.to_vec());
    }

    let removals = crate::renumber::Removals::functions_only(func_removals.clone());
    let index_map = crate::renumber::build_index_map(
        graph.num_functions,
        &HashMap::new(),
        &func_removals,
    );
    let mut reencoder = crate::renumber::ModuleRenumberer::function_only(index_map);
    let counts = crate::rume::ModuleCounts {
        num_functions: graph.num_functions,
        num_tables: 0,
        num_memories: 0,
        num_globals: 0,
        num_func_imports: graph.num_imports,
        num_table_imports: 0,
        num_memory_imports: 0,
        num_global_imports: 0,
    };
    crate::renumber::rebuild_module(module_bytes, &mut reencoder, &removals, &counts)
}
```

- [ ] **Step 3: Update lib.rs tests**

Update `OptimizeConfig` in tests to include `rume`:

```rust
    fn optimize_reduces_component_size() {
        ...
        let config = OptimizeConfig { dce: true, dfe: true, rume: true };
        ...
    }

    fn optimize_with_all_disabled_passes_through() {
        ...
        let config = OptimizeConfig { dce: false, dfe: false, rume: false };
        ...
    }
```

- [ ] **Step 4: Run all tests**

Run: `cargo test -p vela-core`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/vela-core/src/lib.rs crates/vela-core/src/dce.rs
git commit -m "feat: integrate RUME into optimize_module pipeline"
```

---

### Task 4: CLI `--no-rume` Flag

**Files:**
- Modify: `crates/vela-cli/src/main.rs`

- [ ] **Step 1: Add `no_rume` to CLI**

Add to the `Optimize` variant:
```rust
        /// Disable Remove Unused Module Elements
        #[arg(long)]
        no_rume: bool,
```

Update the match arm:
```rust
        Commands::Optimize { input, output, no_dce, no_dfe, no_rume } => {
            ...
            let config = vela_core::OptimizeConfig { dce: !no_dce, dfe: !no_dfe, rume: !no_rume };
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Run: `cargo run -- optimize --help`
Expected: Shows `--no-rume` flag.

- [ ] **Step 3: Commit**

```bash
git add crates/vela-cli/src/main.rs
git commit -m "feat: add --no-rume CLI flag"
```

---

### Task 5: Integration Tests — RUME with wasmtime

**Files:**
- Modify: `crates/vela-core/tests/integration_test.rs`

- [ ] **Step 1: Update existing tests for new `OptimizeConfig`**

Add `rume: true` / `rume: false` to all existing `OptimizeConfig` instances.

- [ ] **Step 2: Add RUME integration test**

```rust
fn build_component_with_unused_global_wat() -> Vec<u8> {
    wat::parse_str(
        r#"
        (component
            (core module $m
                (global $used (mut i32) (i32.const 0))
                (global $unused (mut i32) (i32.const 999))
                (func $get (export "get") (result i32)
                    global.get $used
                )
                (func $set (export "set") (param i32)
                    local.get 0
                    global.set $used
                )
            )
            (core instance $i (instantiate $m))
            (func (export "get") (result u32)
                (canon lift (core func $i "get"))
            )
            (func (export "set") (param "val" u32)
                (canon lift (core func $i "set"))
            )
        )
    "#,
    )
    .expect("WAT should parse")
}

#[test]
fn rume_removes_unused_global_and_runs() {
    let original = build_component_with_unused_global_wat();
    let optimized = optimize(
        &original,
        &OptimizeConfig { dce: true, dfe: true, rume: true },
    )
    .expect("optimize should succeed");

    assert!(optimized.len() < original.len());

    // Verify the optimized component still works
    assert_eq!(call_func(&optimized, "get"), 0);
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test -p vela-core --test integration_test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/vela-core/tests/integration_test.rs
git commit -m "test: RUME integration test with wasmtime"
```

---

## Summary

| Task | Description | Key Output |
|------|-------------|------------|
| 1 | Usage analysis | `analyze_usage`, `UsageInfo`, `ModuleCounts` |
| 2 | ModuleRenumberer | Extended Reencode impl + rebuild_module for all index spaces |
| 3 | Pipeline integration | `optimize_module` with RUME pass |
| 4 | CLI update | `--no-rume` flag |
| 5 | Integration tests | wasmtime verification of RUME correctness |
