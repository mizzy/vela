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

/// Build a core module with duplicate functions:
/// - func 0: exported "entry", calls func 1 and func 3
/// - func 1: () -> i32, returns 42
/// - func 2: dead leaf
/// - func 3: () -> i32, returns 42 (duplicate of func 1)
pub fn build_module_with_duplicates() -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![]);         // type 0: () -> ()
    types.ty().function(vec![], vec![ValType::I32]); // type 1: () -> i32
    module.section(&types);

    let mut functions = FunctionSection::new();
    functions.function(0); // func 0: () -> ()
    functions.function(1); // func 1: () -> i32
    functions.function(0); // func 2: () -> ()  (dead)
    functions.function(1); // func 3: () -> i32 (duplicate of func 1)
    module.section(&functions);

    let mut exports = ExportSection::new();
    exports.export("entry", ExportKind::Func, 0);
    module.section(&exports);

    let mut codes = CodeSection::new();

    // func 0: calls func 1 and func 3
    let mut f0 = Function::new(vec![]);
    f0.instruction(&Instruction::Call(1));
    f0.instruction(&Instruction::Drop);
    f0.instruction(&Instruction::Call(3));
    f0.instruction(&Instruction::Drop);
    f0.instruction(&Instruction::End);
    codes.function(&f0);

    // func 1: returns 42
    let mut f1 = Function::new(vec![]);
    f1.instruction(&Instruction::I32Const(42));
    f1.instruction(&Instruction::End);
    codes.function(&f1);

    // func 2: dead leaf
    let mut f2 = Function::new(vec![]);
    f2.instruction(&Instruction::End);
    codes.function(&f2);

    // func 3: returns 42 (same type and body as func 1)
    let mut f3 = Function::new(vec![]);
    f3.instruction(&Instruction::I32Const(42));
    f3.instruction(&Instruction::End);
    codes.function(&f3);

    module.section(&codes);
    module.finish()
}
