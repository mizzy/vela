// crates/vela-core/src/dce.rs
use std::collections::{HashMap, HashSet};
use crate::callgraph::CallGraph;
use crate::error::VelaError;

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
    let mut reachable = HashSet::with_capacity(graph.num_functions as usize);
    let mut stack: Vec<u32> = Vec::with_capacity(roots.len());
    stack.extend(roots);

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

/// Apply DCE: fully remove unreachable functions and renumber indices.
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

    let removals_set = crate::renumber::Removals::functions_only(removals);
    let index_map = crate::renumber::build_index_map(
        graph.num_functions,
        &HashMap::new(),
        &removals_set.functions,
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
    crate::renumber::rebuild_module(module_bytes, &mut reencoder, &removals_set, &counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::build_basic_module;
    use std::borrow::Cow;
    use wasm_encoder::*;

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
    fn eliminates_dead_functions() {
        let wasm = build_basic_module();
        let optimized = eliminate_dead_functions(&wasm).expect("should optimize");

        wasmparser::Validator::new().validate_all(&optimized).expect("optimized module should be valid");

        // Dead func 3 should be fully removed — only 3 functions remain
        let graph = CallGraph::from_module(&optimized).expect("should parse optimized module");
        assert_eq!(graph.num_functions, 3, "dead function should be fully removed");

        assert!(optimized.len() < wasm.len(), "optimized should be smaller");
    }

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

        let graph = CallGraph::from_module(&optimized).unwrap();
        assert_eq!(graph.num_functions, 1, "only exported function should remain");

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
}
