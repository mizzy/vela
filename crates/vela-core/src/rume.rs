// crates/vela-core/src/rume.rs
use crate::error::VelaError;
use std::collections::HashSet;

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
///
/// Note: atomic (threads proposal) and SIMD load/store instructions are not
/// currently tracked. If a memory is only referenced via these instructions,
/// it would be incorrectly marked unused. This is acceptable for the current
/// target (Carina provider WASM) but should be addressed for general-purpose use.
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
                    let mut init_reader = global.init_expr.get_operators_reader();
                    while !init_reader.eof() {
                        if let wasmparser::Operator::GlobalGet { global_index } =
                            init_reader.read()?
                        {
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
                        wasmparser::ExternalKind::Table => {
                            used_tables.insert(export.index);
                        }
                        wasmparser::ExternalKind::Memory => {
                            used_memories.insert(export.index);
                        }
                        wasmparser::ExternalKind::Global => {
                            used_globals.insert(export.index);
                        }
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
                        wasmparser::Operator::TableCopy {
                            dst_table,
                            src_table,
                        } => {
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
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
        globals.global(
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
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
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        let mut globals = GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
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

        assert!(
            info.used_globals.contains(&0),
            "exported global should be used"
        );
        assert!(
            info.used_memories.contains(&0),
            "exported memory should be used"
        );
        assert!(
            !info.used_tables.contains(&0),
            "unexported/unreferenced table should be unused"
        );
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
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
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

        assert!(
            !info.used_globals.contains(&0),
            "global only used by dead func should be unused"
        );
    }
}
