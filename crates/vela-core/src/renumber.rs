// crates/vela-core/src/renumber.rs
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;

use crate::error::VelaError;
use wasm_encoder::reencode::{Error, Reencode};

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
    let mut map: Vec<u32> = (0..num_functions).collect();
    for (&from, &to) in redirects {
        map[from as usize] = to;
    }

    let mut compact_map: Vec<u32> = vec![0; num_functions as usize];
    let mut next_index: u32 = 0;
    for i in 0..num_functions {
        if !removals.contains(&i) {
            compact_map[i as usize] = next_index;
            next_index += 1;
        }
    }

    for i in 0..num_functions as usize {
        map[i] = compact_map[map[i] as usize];
    }

    map
}

/// Sets of indices to remove from each index space.
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

/// A re-encoder that remaps indices across all index spaces according to
/// precomputed maps.
pub struct ModuleRenumberer {
    pub function_map: Vec<u32>,
    pub table_map: Option<Vec<u32>>,
    pub memory_map: Option<Vec<u32>>,
    pub global_map: Option<Vec<u32>>,
}

impl ModuleRenumberer {
    pub fn function_only(function_map: Vec<u32>) -> Self {
        Self {
            function_map,
            table_map: None,
            memory_map: None,
            global_map: None,
        }
    }
}

impl Reencode for ModuleRenumberer {
    type Error = Infallible;

    fn function_index(&mut self, func: u32) -> Result<u32, Error<Self::Error>> {
        Ok(self.function_map[func as usize])
    }

    fn table_index(&mut self, table: u32) -> Result<u32, Error<Self::Error>> {
        match &self.table_map {
            Some(map) => Ok(map[table as usize]),
            None => Ok(table),
        }
    }

    fn memory_index(&mut self, memory: u32) -> Result<u32, Error<Self::Error>> {
        match &self.memory_map {
            Some(map) => Ok(map[memory as usize]),
            None => Ok(memory),
        }
    }

    fn global_index(&mut self, global: u32) -> Result<u32, Error<Self::Error>> {
        match &self.global_map {
            Some(map) => Ok(map[global as usize]),
            None => Ok(global),
        }
    }
}

/// Re-encode a core WASM module, removing entries listed in `removals` and
/// remapping all index references according to `reencoder`.
///
/// The function/code sections skip entries for removed function indices.
/// The import section skips imports whose corresponding index is in removals.
/// The table, memory, and global sections skip removed entries.
/// All other sections are re-encoded through `ModuleRenumberer`, which
/// transparently rewrites every index reference.
pub fn rebuild_module(
    module_bytes: &[u8],
    reencoder: &mut ModuleRenumberer,
    removals: &Removals,
    num_imports: &crate::rume::ModuleCounts,
) -> Result<Vec<u8>, VelaError> {
    let enc_err = |e: Error<Infallible>| VelaError::InvalidWasm(format!("{e:?}"));
    let parser = wasmparser::Parser::new(0);
    let mut module = wasm_encoder::Module::new();

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::Version { .. } => {}
            wasmparser::Payload::FunctionSection(reader) => {
                let mut sec = wasm_encoder::FunctionSection::new();
                for (i, type_idx) in reader.into_iter().enumerate() {
                    let func_idx = num_imports.num_func_imports + i as u32;
                    if !removals.functions.contains(&func_idx) {
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
                    let func_idx = num_imports.num_func_imports + code_index as u32;
                    if !removals.functions.contains(&func_idx) {
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
                reencoder
                    .parse_type_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
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
                            let s = removals.functions.contains(&func_idx);
                            func_idx += 1;
                            s
                        }
                        wasmparser::TypeRef::Table(_) => {
                            let s = removals.tables.contains(&table_idx);
                            table_idx += 1;
                            s
                        }
                        wasmparser::TypeRef::Memory(_) => {
                            let s = removals.memories.contains(&memory_idx);
                            memory_idx += 1;
                            s
                        }
                        wasmparser::TypeRef::Global(_) => {
                            let s = removals.globals.contains(&global_idx);
                            global_idx += 1;
                            s
                        }
                        _ => false,
                    };
                    if !should_skip {
                        reencoder.parse_import(&mut sec, import).map_err(&enc_err)?;
                    }
                }
                module.section(&sec);
            }
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
            wasmparser::Payload::MemorySection(reader) => {
                let mut sec = wasm_encoder::MemorySection::new();
                for (i, memory) in reader.into_iter().enumerate() {
                    let idx = num_imports.num_memory_imports + i as u32;
                    if !removals.memories.contains(&idx) {
                        let mem = reencoder.memory_type(memory?).map_err(&enc_err)?;
                        sec.memory(mem);
                    }
                }
                module.section(&sec);
            }
            wasmparser::Payload::GlobalSection(reader) => {
                let mut sec = wasm_encoder::GlobalSection::new();
                for (i, global) in reader.into_iter().enumerate() {
                    let idx = num_imports.num_global_imports + i as u32;
                    if !removals.globals.contains(&idx) {
                        reencoder
                            .parse_global(&mut sec, global?)
                            .map_err(&enc_err)?;
                    }
                }
                module.section(&sec);
            }
            wasmparser::Payload::TagSection(s) => {
                let mut sec = wasm_encoder::TagSection::new();
                reencoder.parse_tag_section(&mut sec, s).map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::ExportSection(s) => {
                let mut sec = wasm_encoder::ExportSection::new();
                reencoder
                    .parse_export_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::StartSection { func, .. } => {
                module.section(&wasm_encoder::StartSection {
                    function_index: reencoder.function_index(func).map_err(&enc_err)?,
                });
            }
            wasmparser::Payload::ElementSection(s) => {
                let mut sec = wasm_encoder::ElementSection::new();
                reencoder
                    .parse_element_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::DataCountSection { count, .. } => {
                module.section(&wasm_encoder::DataCountSection { count });
            }
            wasmparser::Payload::DataSection(s) => {
                let mut sec = wasm_encoder::DataSection::new();
                reencoder
                    .parse_data_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::CustomSection(s) => {
                reencoder
                    .parse_custom_section(&mut module, s)
                    .map_err(&enc_err)?;
            }
            wasmparser::Payload::End(_) => {}
            _ => {}
        }
    }

    Ok(module.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removals_only() {
        let removals = HashSet::from([2, 4]);
        let map = build_index_map(5, &HashMap::new(), &removals);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1);
        assert_eq!(map[3], 2);
    }

    #[test]
    fn redirects_and_removals() {
        let redirects = HashMap::from([(2, 1)]);
        let removals = HashSet::from([2]);
        let map = build_index_map(4, &redirects, &removals);
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
        let redirects = HashMap::from([(1, 0), (2, 0)]);
        let removals = HashSet::from([1, 2]);
        let map = build_index_map(4, &redirects, &removals);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 0);
        assert_eq!(map[2], 0);
        assert_eq!(map[3], 1);
    }

    #[test]
    fn rebuild_removes_dead_function() {
        use wasm_encoder::*;

        // Build module: func 0 (exported "run", calls func 1), func 1 (leaf), func 2 (dead, bigger body)
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

        // func 0: calls func 1
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(1));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        // func 1: leaf
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        // func 2: dead, bigger body
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(1));
        f2.instruction(&Instruction::I32Const(2));
        f2.instruction(&Instruction::I32Add);
        f2.instruction(&Instruction::Drop);
        f2.instruction(&Instruction::I32Const(3));
        f2.instruction(&Instruction::I32Const(4));
        f2.instruction(&Instruction::I32Mul);
        f2.instruction(&Instruction::Drop);
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        module.section(&codes);
        let wasm = module.finish();

        // Remove func 2
        let removals = Removals::functions_only(HashSet::from([2u32]));
        let index_map = build_index_map(3, &HashMap::new(), &removals.functions);
        let mut reencoder = ModuleRenumberer::function_only(index_map);
        let counts = crate::rume::ModuleCounts {
            num_functions: 3,
            num_tables: 0,
            num_memories: 0,
            num_globals: 0,
            num_func_imports: 0,
            num_table_imports: 0,
            num_memory_imports: 0,
            num_global_imports: 0,
        };
        let rebuilt =
            rebuild_module(&wasm, &mut reencoder, &removals, &counts).expect("should succeed");

        // Validate the rebuilt module
        wasmparser::Validator::new()
            .validate_all(&rebuilt)
            .expect("rebuilt module should be valid");

        // Verify 2 functions remain
        let graph = crate::callgraph::CallGraph::from_module(&rebuilt).expect("should parse");
        assert_eq!(
            graph.num_functions, 2,
            "should have 2 functions after removal"
        );

        // Should be smaller than original
        assert!(
            rebuilt.len() < wasm.len(),
            "rebuilt ({}) should be smaller than original ({})",
            rebuilt.len(),
            wasm.len()
        );
    }

    #[test]
    fn rebuild_with_redirect_and_removal() {
        use wasm_encoder::*;

        // Build module:
        // func 0: exported "main", calls func 3
        // func 1: returns i32 42
        // func 2: duplicate of func 1 (same body, returns i32 42)
        // func 3: calls func 2
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(1); // func 0: () -> ()
        functions.function(0); // func 1: () -> i32
        functions.function(0); // func 2: () -> i32
        functions.function(1); // func 3: () -> ()
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();

        // func 0: calls func 3
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(3));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        // func 1: returns 42
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(42));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        // func 2: duplicate of func 1, returns 42
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(42));
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        // func 3: calls func 2, drops result
        let mut f3 = Function::new(vec![]);
        f3.instruction(&Instruction::Call(2));
        f3.instruction(&Instruction::Drop);
        f3.instruction(&Instruction::End);
        codes.function(&f3);

        module.section(&codes);
        let wasm = module.finish();

        // Redirect func 2 -> func 1, remove func 2
        let redirects = HashMap::from([(2u32, 1u32)]);
        let removals = Removals::functions_only(HashSet::from([2u32]));
        let index_map = build_index_map(4, &redirects, &removals.functions);
        let mut reencoder = ModuleRenumberer::function_only(index_map);
        let counts = crate::rume::ModuleCounts {
            num_functions: 4,
            num_tables: 0,
            num_memories: 0,
            num_globals: 0,
            num_func_imports: 0,
            num_table_imports: 0,
            num_memory_imports: 0,
            num_global_imports: 0,
        };
        let rebuilt =
            rebuild_module(&wasm, &mut reencoder, &removals, &counts).expect("should succeed");

        // Validate the rebuilt module
        wasmparser::Validator::new()
            .validate_all(&rebuilt)
            .expect("rebuilt module should be valid");

        // Verify 3 functions remain (0, 1, 3 -> renumbered to 0, 1, 2)
        let graph = crate::callgraph::CallGraph::from_module(&rebuilt).expect("should parse");
        assert_eq!(
            graph.num_functions, 3,
            "should have 3 functions after removal"
        );

        // func 0 (was func 0) should call func 2 (was func 3, compacted)
        assert!(
            graph.edges.get(&0).map_or(false, |c| c.contains(&2)),
            "func 0 should call func 2 (renumbered from func 3)"
        );

        // func 2 (was func 3) should call func 1 (func 2 was redirected to func 1)
        assert!(
            graph.edges.get(&2).map_or(false, |c| c.contains(&1)),
            "func 2 (was func 3) should call func 1 (redirected from func 2)"
        );
    }

    #[test]
    fn rebuild_removes_unused_global() {
        use wasm_encoder::*;

        // Build module with:
        // - global 0: i32, used by func 0 via GlobalGet
        // - global 1: i32, unused
        // - func 0: exported, reads global 0
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0); // func 0: () -> i32
        module.section(&functions);

        let mut globals = GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(42),
        );
        globals.global(
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(99),
        );
        module.section(&globals);

        let mut exports = ExportSection::new();
        exports.export("get_val", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::GlobalGet(0));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        module.section(&codes);
        let wasm = module.finish();

        // Remove global 1
        let removals = Removals {
            functions: HashSet::new(),
            tables: HashSet::new(),
            memories: HashSet::new(),
            globals: HashSet::from([1u32]),
        };

        // Build global index map: global 0 -> 0, global 1 removed
        let global_map = vec![0u32, 0u32]; // global 1 maps to 0 (doesn't matter, it's removed)
        let function_map = vec![0u32]; // single function, identity

        let mut reencoder = ModuleRenumberer {
            function_map,
            table_map: None,
            memory_map: None,
            global_map: Some(global_map),
        };

        let counts = crate::rume::ModuleCounts {
            num_functions: 1,
            num_tables: 0,
            num_memories: 0,
            num_globals: 2,
            num_func_imports: 0,
            num_table_imports: 0,
            num_memory_imports: 0,
            num_global_imports: 0,
        };

        let rebuilt =
            rebuild_module(&wasm, &mut reencoder, &removals, &counts).expect("should succeed");

        // Validate the rebuilt module
        wasmparser::Validator::new()
            .validate_all(&rebuilt)
            .expect("rebuilt module should be valid");

        // Should be smaller (one fewer global)
        assert!(
            rebuilt.len() < wasm.len(),
            "rebuilt ({}) should be smaller than original ({})",
            rebuilt.len(),
            wasm.len()
        );

        // Verify the module still has one global by parsing
        let parser = wasmparser::Parser::new(0);
        let mut global_count = 0u32;
        for payload in parser.parse_all(&rebuilt) {
            if let wasmparser::Payload::GlobalSection(reader) = payload.unwrap() {
                global_count = reader.count();
            }
        }
        assert_eq!(global_count, 1, "should have 1 global after removal");
    }

    #[test]
    fn rebuild_removes_unused_global_import() {
        use wasm_encoder::*;

        // Module with:
        // - imported global 0 "env"."used_g" (used by func 0)
        // - imported global 1 "env"."unused_g" (unused)
        // - func 0: exported, reads global 0
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import(
            "env",
            "used_g",
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
        );
        imports.import(
            "env",
            "unused_g",
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
        );
        module.section(&imports);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

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

        // Remove global 1 (unused import)
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
            table_map: None,
            memory_map: None,
            global_map: Some(global_map),
        };
        let counts = crate::rume::ModuleCounts {
            num_functions: 1,
            num_tables: 0,
            num_memories: 0,
            num_globals: 2,
            num_func_imports: 0,
            num_table_imports: 0,
            num_memory_imports: 0,
            num_global_imports: 2,
        };
        let rebuilt =
            rebuild_module(&wasm, &mut reencoder, &removals, &counts).expect("should succeed");

        wasmparser::Validator::new()
            .validate_all(&rebuilt)
            .expect("should be valid");

        // Verify only 1 import remains
        let parser = wasmparser::Parser::new(0);
        let mut import_count = 0u32;
        for payload in parser.parse_all(&rebuilt) {
            if let wasmparser::Payload::ImportSection(reader) = payload.unwrap() {
                for import in reader.into_imports() {
                    import.unwrap();
                    import_count += 1;
                }
            }
        }
        assert_eq!(
            import_count, 1,
            "should have 1 import after removing unused global import"
        );
    }
}
