// crates/vela-core/src/callgraph.rs
use std::collections::{HashMap, HashSet};
use crate::error::VelaError;

#[derive(Debug)]
pub struct CallGraph {
    pub edges: HashMap<u32, HashSet<u32>>,
    pub num_functions: u32,
    pub num_imports: u32,
}

impl CallGraph {
    pub fn from_module(module_bytes: &[u8]) -> Result<Self, VelaError> {
        let parser = wasmparser::Parser::new(0);
        let mut edges: HashMap<u32, HashSet<u32>> = HashMap::new();
        let mut num_imports: u32 = 0;
        let mut num_functions: u32 = 0;
        let mut code_index: u32 = 0;

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
                    num_functions = num_imports + reader.count();
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let func_index = num_imports + code_index;
                    let callees = edges.entry(func_index).or_default();

                    let mut ops = body.get_operators_reader()?;
                    while !ops.eof() {
                        match ops.read()? {
                            wasmparser::Operator::Call { function_index } => {
                                callees.insert(function_index);
                            }
                            wasmparser::Operator::ReturnCall { function_index } => {
                                callees.insert(function_index);
                            }
                            // Track ref.func as an edge — the referenced function
                            // could be called via call_ref or placed in a table
                            wasmparser::Operator::RefFunc { function_index } => {
                                callees.insert(function_index);
                            }
                            // Note: call_indirect targets are resolved at runtime via
                            // table lookup. We handle this conservatively by marking all
                            // elem-section functions as roots in find_roots(). Functions
                            // placed in tables via host code or table.set at runtime are
                            // not tracked — this is a known limitation.
                            _ => {}
                        }
                    }
                    code_index += 1;
                }
                _ => {}
            }
        }

        Ok(CallGraph {
            edges,
            num_functions,
            num_imports,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::build_basic_module;
    use wasm_encoder::*;

    fn build_module_with_imports() -> Vec<u8> {
        // Build a module with 1 import + 2 defined functions:
        // func 0: import "env"."log"
        // func 1: defined, calls func 0
        // func 2: defined, leaf
        let mut module = Module::new();

        // Type section: one type () -> ()
        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        // Import section: import "env"."log" as func type 0
        let mut imports = ImportSection::new();
        imports.import("env", "log", EntityType::Function(0));
        module.section(&imports);

        // Function section: 2 defined functions using type 0
        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(0);
        module.section(&functions);

        // Code section
        let mut codes = CodeSection::new();

        // func 1 (index 1): calls func 0 (the import)
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::Call(0));
        f1.instruction(&Instruction::End);
        codes.function(&f1);

        // func 2 (index 2): leaf
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        codes.function(&f2);

        module.section(&codes);
        module.finish()
    }

    #[test]
    fn builds_call_graph_from_module() {
        let wasm = build_basic_module();
        let cg = CallGraph::from_module(&wasm).expect("should parse");

        assert_eq!(cg.num_imports, 0);
        assert_eq!(cg.num_functions, 4);

        // func 0 calls func 1
        assert!(cg.edges[&0].contains(&1));
        assert_eq!(cg.edges[&0].len(), 1);

        // func 1 calls func 2
        assert!(cg.edges[&1].contains(&2));
        assert_eq!(cg.edges[&1].len(), 1);

        // func 2 is a leaf
        assert!(cg.edges[&2].is_empty());

        // func 3 (dead) calls func 2
        assert!(cg.edges[&3].contains(&2));
        assert_eq!(cg.edges[&3].len(), 1);
    }

    #[test]
    fn handles_imports() {
        let wasm = build_module_with_imports();
        let cg = CallGraph::from_module(&wasm).expect("should parse");

        assert_eq!(cg.num_imports, 1);
        assert_eq!(cg.num_functions, 3);

        // func 1 (first defined) calls func 0 (the import)
        assert!(cg.edges[&1].contains(&0));
        assert_eq!(cg.edges[&1].len(), 1);

        // func 2 is a leaf
        assert!(cg.edges[&2].is_empty());
    }
}
