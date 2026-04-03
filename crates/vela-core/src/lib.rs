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
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { dce: true, dfe: true }
    }
}

/// Optimize a Component Model WASM binary.
pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    component::process_component(wasm, |module_bytes| {
        optimize_module(module_bytes, config)
    })
}

fn optimize_module(module_bytes: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    if !config.dce && !config.dfe {
        return Ok(module_bytes.to_vec());
    }

    let graph = callgraph::CallGraph::from_module(module_bytes)?;
    let roots = dce::find_roots(module_bytes, &graph)?;
    let reachable = dce::find_reachable(&roots, &graph);

    let mut removals: HashSet<u32> = if config.dce {
        (0..graph.num_functions)
            .filter(|i| !reachable.contains(i))
            .collect()
    } else {
        HashSet::new()
    };

    let mut redirects: HashMap<u32, u32> = HashMap::new();
    if config.dfe {
        let dfe_result = dfe::find_duplicates(module_bytes, &reachable)?;
        redirects = dfe_result.redirects;
        removals.extend(dfe_result.removals);
    }

    if removals.is_empty() && redirects.is_empty() {
        return Ok(module_bytes.to_vec());
    }

    let index_map = renumber::build_index_map(graph.num_functions, &redirects, &removals);
    renumber::rebuild_module(module_bytes, &index_map, &removals, graph.num_imports)
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
        let config = OptimizeConfig { dce: true, dfe: true };
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
        let config = OptimizeConfig { dce: false, dfe: false };
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
