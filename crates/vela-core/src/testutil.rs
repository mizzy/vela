/// Shared test helpers for building WASM modules.
use wasm_encoder::*;

/// Build a core module with 4 functions (no imports):
/// - func 0: exported "entry", calls func 1
/// - func 1: calls func 2
/// - func 2: leaf
/// - func 3: dead, calls func 2
pub fn build_basic_module() -> Vec<u8> {
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
