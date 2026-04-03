// crates/vela-core/src/lib.rs
pub mod callgraph;
pub mod component;
pub mod dce;
pub mod dfe;
pub mod error;
pub mod renumber;
pub mod rume;
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
    /// Enable Redundant/Unused Member Elimination (tables, memories, globals).
    pub rume: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { dce: true, dfe: true, rume: true }
    }
}

/// Optimize a Component Model WASM binary.
pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    component::process_component(wasm, |module_bytes| {
        optimize_module(module_bytes, config)
    })
}

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

    // RUME: analyze usage to get counts and determine unused elements
    let usage = rume::analyze_usage(module_bytes, &reachable)?;
    let counts = &usage.counts;
    debug_assert_eq!(graph.num_functions, counts.num_functions);

    let (table_removals, memory_removals, global_removals) = if config.rume {
        (
            (0..counts.num_tables).filter(|i| !usage.used_tables.contains(i)).collect(),
            (0..counts.num_memories).filter(|i| !usage.used_memories.contains(i)).collect(),
            (0..counts.num_globals).filter(|i| !usage.used_globals.contains(i)).collect(),
        )
    } else {
        (HashSet::new(), HashSet::new(), HashSet::new())
    };

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

    let table_map = if removals.tables.is_empty() { None } else {
        Some(renumber::build_index_map(counts.num_tables, &HashMap::new(), &removals.tables))
    };
    let memory_map = if removals.memories.is_empty() { None } else {
        Some(renumber::build_index_map(counts.num_memories, &HashMap::new(), &removals.memories))
    };
    let global_map = if removals.globals.is_empty() { None } else {
        Some(renumber::build_index_map(counts.num_globals, &HashMap::new(), &removals.globals))
    };

    let mut reencoder = renumber::ModuleRenumberer {
        function_map: func_map,
        table_map,
        memory_map,
        global_map,
    };

    renumber::rebuild_module(module_bytes, &mut reencoder, &removals, counts)
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
        let config = OptimizeConfig { dce: true, dfe: true, rume: true };
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
        let config = OptimizeConfig { dce: false, dfe: false, rume: false };
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
