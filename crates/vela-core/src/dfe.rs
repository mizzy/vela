// crates/vela-core/src/dfe.rs
use crate::error::VelaError;
use std::collections::{HashMap, HashSet};

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
    use std::hash::{Hash, Hasher};

    let parser = wasmparser::Parser::new(0);
    let mut num_imports: u32 = 0;
    let mut type_indices: Vec<u32> = Vec::new();
    // Store byte ranges into module_bytes instead of cloning body bytes
    let mut body_ranges: Vec<std::ops::Range<usize>> = Vec::new();

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
                body_ranges.push(body.range());
            }
            _ => {}
        }
    }

    // key → (representative func_index, body range for byte comparison on hash collision)
    let mut groups: HashMap<(u32, u64), (u32, std::ops::Range<usize>)> = HashMap::new();
    let mut redirects = HashMap::new();
    let mut removals = HashSet::new();

    for (code_idx, range) in body_ranges.iter().enumerate() {
        let func_idx = num_imports + code_idx as u32;
        if !reachable.contains(&func_idx) {
            continue;
        }

        let bytes = &module_bytes[range.start..range.end];
        let type_idx = type_indices[code_idx];
        let mut hasher = std::hash::DefaultHasher::new();
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        let key = (type_idx, hash);

        match groups.get(&key) {
            Some((representative, rep_range))
                if module_bytes[rep_range.start..rep_range.end] == *bytes =>
            {
                redirects.insert(func_idx, *representative);
                removals.insert(func_idx);
            }
            Some(_) => {
                // Hash collision with different body — treated as unique.
                // Known limitation: negligible probability with DefaultHasher.
            }
            None => {
                groups.insert(key, (func_idx, range.clone()));
            }
        }
    }

    Ok(DfeResult {
        redirects,
        removals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callgraph::CallGraph;
    use crate::dce::{find_reachable, find_roots};
    use crate::testutil::build_module_with_duplicates;

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
        // func 1: () -> i32, body is i32.const(42) + end
        // func 2: () -> (), body is i32.const(42) + end (DIFFERENT TYPE from func 1)
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0); // func 0
        functions.function(1); // func 1: () -> i32
        functions.function(0); // func 2: () -> () (different type!)
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
        assert!(
            result.redirects.is_empty(),
            "different types should not be duplicates"
        );
    }
}
