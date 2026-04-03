// crates/vela-core/src/dce.rs
use std::collections::HashSet;
use crate::callgraph::CallGraph;
use crate::error::VelaError;
use wasm_encoder::reencode::{Reencode, RoundtripReencoder};

pub fn find_roots(module_bytes: &[u8], graph: &CallGraph) -> Result<HashSet<u32>, VelaError> {
    let parser = wasmparser::Parser::new(0);
    let mut roots = HashSet::new();

    for i in 0..graph.num_imports {
        roots.insert(i);
    }

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    if matches!(export.kind, wasmparser::ExternalKind::Func) {
                        roots.insert(export.index);
                    }
                }
            }
            wasmparser::Payload::StartSection { func, .. } => {
                roots.insert(func);
            }
            wasmparser::Payload::ElementSection(reader) => {
                for elem in reader {
                    let elem = elem?;
                    match elem.items {
                        wasmparser::ElementItems::Functions(reader) => {
                            for idx in reader {
                                roots.insert(idx?);
                            }
                        }
                        wasmparser::ElementItems::Expressions(_, reader) => {
                            for expr in reader {
                                let expr = expr?;
                                let mut ops = expr.get_operators_reader();
                                while !ops.eof() {
                                    if let wasmparser::Operator::RefFunc { function_index } =
                                        ops.read()?
                                    {
                                        roots.insert(function_index);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(roots)
}

pub fn find_reachable(roots: &HashSet<u32>, graph: &CallGraph) -> HashSet<u32> {
    let mut reachable = HashSet::new();
    let mut stack: Vec<u32> = roots.iter().copied().collect();

    while let Some(func) = stack.pop() {
        if !reachable.insert(func) {
            continue;
        }
        if let Some(callees) = graph.edges.get(&func) {
            for &callee in callees {
                if !reachable.contains(&callee) {
                    stack.push(callee);
                }
            }
        }
    }

    reachable
}

pub fn eliminate_dead_code(module_bytes: &[u8]) -> Result<Vec<u8>, VelaError> {
    let graph = CallGraph::from_module(module_bytes)?;
    let roots = find_roots(module_bytes, &graph)?;
    let reachable = find_reachable(&roots, &graph);

    let parser = wasmparser::Parser::new(0);
    let mut module = wasm_encoder::Module::new();
    let mut reencoder = RoundtripReencoder;
    let orig_offset = parser.offset() as usize;

    let get_original_section = |range: std::ops::Range<usize>| -> Result<&[u8], VelaError> {
        module_bytes
            .get(range.start - orig_offset..range.end - orig_offset)
            .ok_or_else(|| VelaError::InvalidWasm("invalid code section range".into()))
    };

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::Version { .. } => {}
            wasmparser::Payload::CodeSectionStart { range, .. } => {
                let section_bytes = get_original_section(range.clone())?;
                let reader = wasmparser::BinaryReader::new(section_bytes, range.start);
                let code_reader = wasmparser::CodeSectionReader::new(reader)
                    .map_err(|e| VelaError::Wasm(e))?;

                let mut code_section = wasm_encoder::CodeSection::new();
                let mut code_index: u32 = 0;
                for func_result in code_reader {
                    let func_body = func_result?;
                    let func_index = graph.num_imports + code_index;

                    if reachable.contains(&func_index) {
                        // Re-encode the live function body faithfully
                        reencoder
                            .parse_function_body(&mut code_section, func_body)
                            .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                    } else {
                        // Replace dead function body with unreachable + end
                        let mut f = wasm_encoder::Function::new(vec![]);
                        f.instruction(&wasm_encoder::Instruction::Unreachable);
                        f.instruction(&wasm_encoder::Instruction::End);
                        code_section.function(&f);
                    }
                    code_index += 1;
                }
                module.section(&code_section);
            }
            wasmparser::Payload::CodeSectionEntry(_) => {
                // Handled above via CodeSectionStart
            }
            wasmparser::Payload::TypeSection(section) => {
                let mut types = wasm_encoder::TypeSection::new();
                reencoder
                    .parse_type_section(&mut types, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&types);
            }
            wasmparser::Payload::ImportSection(section) => {
                let mut imports = wasm_encoder::ImportSection::new();
                reencoder
                    .parse_import_section(&mut imports, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&imports);
            }
            wasmparser::Payload::FunctionSection(section) => {
                let mut functions = wasm_encoder::FunctionSection::new();
                reencoder
                    .parse_function_section(&mut functions, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&functions);
            }
            wasmparser::Payload::TableSection(section) => {
                let mut tables = wasm_encoder::TableSection::new();
                reencoder
                    .parse_table_section(&mut tables, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&tables);
            }
            wasmparser::Payload::MemorySection(section) => {
                let mut memories = wasm_encoder::MemorySection::new();
                reencoder
                    .parse_memory_section(&mut memories, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&memories);
            }
            wasmparser::Payload::TagSection(section) => {
                let mut tags = wasm_encoder::TagSection::new();
                reencoder
                    .parse_tag_section(&mut tags, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&tags);
            }
            wasmparser::Payload::GlobalSection(section) => {
                let mut globals = wasm_encoder::GlobalSection::new();
                reencoder
                    .parse_global_section(&mut globals, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&globals);
            }
            wasmparser::Payload::ExportSection(section) => {
                let mut exports = wasm_encoder::ExportSection::new();
                reencoder
                    .parse_export_section(&mut exports, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&exports);
            }
            wasmparser::Payload::StartSection { func, range: _ } => {
                module.section(&wasm_encoder::StartSection {
                    function_index: func,
                });
            }
            wasmparser::Payload::ElementSection(section) => {
                let mut elements = wasm_encoder::ElementSection::new();
                reencoder
                    .parse_element_section(&mut elements, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&elements);
            }
            wasmparser::Payload::DataCountSection { count, range: _ } => {
                module.section(&wasm_encoder::DataCountSection { count });
            }
            wasmparser::Payload::DataSection(section) => {
                let mut data = wasm_encoder::DataSection::new();
                reencoder
                    .parse_data_section(&mut data, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
                module.section(&data);
            }
            wasmparser::Payload::CustomSection(section) => {
                reencoder
                    .parse_custom_section(&mut module, section)
                    .map_err(|e| VelaError::InvalidWasm(format!("{e:?}")))?;
            }
            wasmparser::Payload::End(_) => {}
            _ => {
                // Skip unknown/component sections
            }
        }
    }

    Ok(module.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use wasm_encoder::*;

    /// Build a module with 4 functions (no imports):
    /// func 0: exported "entry", calls func 1
    /// func 1: calls func 2
    /// func 2: leaf
    /// func 3: dead, calls func 2
    fn build_basic_module() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(0);
        functions.function(0);
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("entry", ExportKind::Func, 0);
        module.section(&exports);

        let mut codes = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(1));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::Call(2));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        let mut f3 = Function::new(vec![]);
        f3.instruction(&Instruction::Call(2));
        f3.instruction(&Instruction::End);
        codes.function(&f3);

        module.section(&codes);
        module.finish()
    }

    /// Build a module with:
    /// func 0: start function, no calls
    /// func 1: referenced in elem section, no calls
    /// func 2: dead, leaf
    fn build_start_and_elem_module() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(0);
        functions.function(0);
        module.section(&functions);

        let mut tables = TableSection::new();
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            minimum: 10,
            maximum: None,
            table64: false,
            shared: false,
        });
        module.section(&tables);

        // StartSection must come before ElementSection in WASM binary format
        module.section(&StartSection { function_index: 0 });

        let mut elements = ElementSection::new();
        let offset = ConstExpr::i32_const(0);
        elements.active(
            Some(0),
            &offset,
            Elements::Functions(Cow::Borrowed(&[1u32])),
        );
        module.section(&elements);

        let mut codes = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        module.section(&codes);
        module.finish()
    }

    #[test]
    fn finds_exported_roots() {
        let wasm = build_basic_module();
        let graph = CallGraph::from_module(&wasm).expect("should parse");
        let roots = find_roots(&wasm, &graph).expect("should find roots");

        // func 0 is exported as "entry"
        assert!(roots.contains(&0), "exported func 0 should be a root");
        // func 3 is dead and not exported
        assert!(!roots.contains(&3), "dead func 3 should not be a root");
        // func 1 and 2 are not roots (only reachable via call graph)
        assert!(!roots.contains(&1), "func 1 should not be a root");
        assert!(!roots.contains(&2), "func 2 should not be a root");
    }

    #[test]
    fn finds_reachable_functions() {
        let wasm = build_basic_module();
        let graph = CallGraph::from_module(&wasm).expect("should parse");
        let roots = find_roots(&wasm, &graph).expect("should find roots");
        let reachable = find_reachable(&roots, &graph);

        // func 0 exported -> calls func 1 -> calls func 2: all reachable
        assert!(reachable.contains(&0), "func 0 should be reachable");
        assert!(reachable.contains(&1), "func 1 should be reachable");
        assert!(reachable.contains(&2), "func 2 should be reachable");
        // func 3 is dead: not reachable from any root
        assert!(!reachable.contains(&3), "func 3 should not be reachable");
    }

    #[test]
    fn start_and_elem_are_roots() {
        let wasm = build_start_and_elem_module();
        let graph = CallGraph::from_module(&wasm).expect("should parse");
        let roots = find_roots(&wasm, &graph).expect("should find roots");

        // func 0 is the start function
        assert!(roots.contains(&0), "start func 0 should be a root");
        // func 1 is referenced in the elem section
        assert!(roots.contains(&1), "elem-referenced func 1 should be a root");
        // func 2 is dead
        assert!(!roots.contains(&2), "dead func 2 should not be a root");
    }

    #[test]
    fn eliminates_dead_function_body() {
        let wasm = build_basic_module();
        let optimized = eliminate_dead_code(&wasm).expect("should optimize");

        // Optimized module should be smaller (dead func 3 body replaced with unreachable)
        assert!(
            optimized.len() <= wasm.len(),
            "optimized module should not be larger than original"
        );

        // Validate the optimized module
        wasmparser::Validator::new().validate_all(&optimized).expect("optimized module should be valid");

        // Verify dead func 3 has no outgoing calls in the optimized module
        let graph = CallGraph::from_module(&optimized).expect("should parse optimized module");
        assert!(
            graph.edges.get(&3).map_or(true, |callees| callees.is_empty()),
            "dead func 3 should have no outgoing calls after DCE"
        );
    }

    #[test]
    fn live_functions_preserved_correctly() {
        // Build a module with func 0 (exported, returns i32 42) and func 1 (dead)
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

        // func 0: returns i32.const 42
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::I32Const(42));
        f0.instruction(&Instruction::End);
        codes.function(&f0);

        // func 1: dead, returns i32.const 99
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(99));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        module.section(&codes);
        let wasm = module.finish();

        let optimized = eliminate_dead_code(&wasm).expect("should optimize");

        // Validate
        wasmparser::Validator::new().validate_all(&optimized).expect("optimized module should be valid");

        // Verify func 0 still contains i32.const 42
        let parser = wasmparser::Parser::new(0);
        let mut code_index = 0u32;
        let mut found_42 = false;
        for payload in parser.parse_all(&optimized) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                if code_index == 0 {
                    let mut ops = body.get_operators_reader().unwrap();
                    while !ops.eof() {
                        if let wasmparser::Operator::I32Const { value: 42 } = ops.read().unwrap() {
                            found_42 = true;
                        }
                    }
                }
                code_index += 1;
            }
        }
        assert!(found_42, "func 0 should still contain i32.const 42 after DCE");
    }
}
