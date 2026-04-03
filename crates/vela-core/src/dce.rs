// crates/vela-core/src/dce.rs
use std::collections::HashSet;
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
}
