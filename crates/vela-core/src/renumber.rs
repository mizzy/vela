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

/// A re-encoder that remaps function indices according to a precomputed map.
///
/// All other index spaces (types, globals, etc.) are passed through unchanged
/// via the default `Reencode` implementations (which delegate to `RoundtripReencoder`
/// identity behavior).
pub struct FunctionRenumberer {
    pub index_map: Vec<u32>,
}

impl Reencode for FunctionRenumberer {
    type Error = Infallible;

    fn function_index(&mut self, func: u32) -> Result<u32, Error<Self::Error>> {
        Ok(self.index_map[func as usize])
    }
}

/// Re-encode a core WASM module, removing functions listed in `removals` and
/// remapping all function index references according to `index_map`.
///
/// The function section and code section are handled specially: entries
/// corresponding to removed function indices are skipped entirely. All other
/// sections are re-encoded through `FunctionRenumberer`, which transparently
/// rewrites every function index reference (call instructions, exports,
/// element segments, ref.func, etc.).
///
/// Note: re-encoding may produce slightly different (sometimes larger) LEB128
/// encodings than the original. When very few functions are removed, this
/// overhead can exceed the savings. This is expected and not a bug.
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
                reencoder
                    .parse_type_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::ImportSection(s) => {
                let mut sec = wasm_encoder::ImportSection::new();
                reencoder
                    .parse_import_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::TableSection(s) => {
                let mut sec = wasm_encoder::TableSection::new();
                reencoder
                    .parse_table_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::MemorySection(s) => {
                let mut sec = wasm_encoder::MemorySection::new();
                reencoder
                    .parse_memory_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::TagSection(s) => {
                let mut sec = wasm_encoder::TagSection::new();
                reencoder
                    .parse_tag_section(&mut sec, s)
                    .map_err(&enc_err)?;
                module.section(&sec);
            }
            wasmparser::Payload::GlobalSection(s) => {
                let mut sec = wasm_encoder::GlobalSection::new();
                reencoder
                    .parse_global_section(&mut sec, s)
                    .map_err(&enc_err)?;
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
        let removals = HashSet::from([2u32]);
        let index_map = build_index_map(3, &HashMap::new(), &removals);
        let rebuilt = rebuild_module(&wasm, &index_map, &removals, 0).expect("rebuild should succeed");

        // Validate the rebuilt module
        wasmparser::Validator::new()
            .validate_all(&rebuilt)
            .expect("rebuilt module should be valid");

        // Verify 2 functions remain
        let graph = crate::callgraph::CallGraph::from_module(&rebuilt).expect("should parse");
        assert_eq!(graph.num_functions, 2, "should have 2 functions after removal");

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
        let removals = HashSet::from([2u32]);
        let index_map = build_index_map(4, &redirects, &removals);
        let rebuilt = rebuild_module(&wasm, &index_map, &removals, 0).expect("rebuild should succeed");

        // Validate the rebuilt module
        wasmparser::Validator::new()
            .validate_all(&rebuilt)
            .expect("rebuilt module should be valid");

        // Verify 3 functions remain (0, 1, 3 -> renumbered to 0, 1, 2)
        let graph = crate::callgraph::CallGraph::from_module(&rebuilt).expect("should parse");
        assert_eq!(graph.num_functions, 3, "should have 3 functions after removal");

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
}
