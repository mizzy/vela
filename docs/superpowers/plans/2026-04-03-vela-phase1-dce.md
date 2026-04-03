# Vela Phase 1: Dead Code Elimination — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI tool that performs Dead Code Elimination on WASM Component Model binaries, replacing unreachable function bodies with `unreachable` instructions to reduce binary size.

**Architecture:** Workspace with two crates — `vela-core` (library: parse Component WASM, extract core modules, build call graph, identify dead functions, reconstruct with dead bodies replaced) and `vela-cli` (binary: thin CLI wrapper using clap). Uses wasmparser for zero-copy parsing and wasm-encoder's reencode module for faithful round-trip reconstruction with surgical modifications.

**Tech Stack:** Rust 1.93+, wasmparser 0.246, wasm-encoder 0.246, clap 4, wasmtime 43 (dev/test only)

---

## File Map

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Workspace root defining members |
| `crates/vela-core/Cargo.toml` | Library crate dependencies |
| `crates/vela-core/src/lib.rs` | Public API: `optimize()`, `OptimizeConfig`, re-exports |
| `crates/vela-core/src/error.rs` | `VelaError` enum |
| `crates/vela-core/src/component.rs` | Component-level traversal: extract core modules, reconstruct component |
| `crates/vela-core/src/callgraph.rs` | Call graph construction from a core module's code section |
| `crates/vela-core/src/dce.rs` | DCE pass: root identification, reachability, body replacement |
| `crates/vela-cli/Cargo.toml` | CLI binary crate dependencies |
| `crates/vela-cli/src/main.rs` | CLI entry point with `optimize` and `info` subcommands |
| `tests/integration/dce_test.rs` | End-to-end: build Component WASM, optimize, run in wasmtime |
| `.gitignore` | Ignore build artifacts, benchmark fixtures |

---

### Task 1: Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `crates/vela-core/Cargo.toml`
- Create: `crates/vela-core/src/lib.rs`
- Create: `crates/vela-cli/Cargo.toml`
- Create: `crates/vela-cli/src/main.rs`
- Create: `.gitignore`

- [ ] **Step 1: Create workspace root Cargo.toml**

```toml
# Cargo.toml
[workspace]
members = ["crates/vela-core", "crates/vela-cli"]
resolver = "2"
```

- [ ] **Step 2: Create vela-core Cargo.toml**

```toml
# crates/vela-core/Cargo.toml
[package]
name = "vela-core"
version = "0.1.0"
edition = "2021"

[dependencies]
wasmparser = "0.246"
wasm-encoder = "0.246"
thiserror = "2"

[dev-dependencies]
wasmtime = "43"
wasmtime-wasi = "43"
wat = "1"
```

- [ ] **Step 3: Create vela-core lib.rs placeholder**

```rust
// crates/vela-core/src/lib.rs
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(add(2, 2), 4);
    }
}
```

- [ ] **Step 4: Create vela-cli Cargo.toml**

```toml
# crates/vela-cli/Cargo.toml
[package]
name = "vela-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "vela"
path = "src/main.rs"

[dependencies]
vela-core = { path = "../vela-core" }
clap = { version = "4", features = ["derive"] }
```

- [ ] **Step 5: Create vela-cli main.rs placeholder**

```rust
// crates/vela-cli/src/main.rs
fn main() {
    println!("vela: WASM Component Model optimizer");
}
```

- [ ] **Step 6: Create .gitignore**

```
/target
benches/fixtures/
```

- [ ] **Step 7: Build and verify**

Run: `cargo build`
Expected: Compiles successfully, downloads dependencies.

Run: `cargo test`
Expected: 1 test passes (`it_works`).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/ .gitignore
git commit -m "feat: scaffold workspace with vela-core and vela-cli"
```

---

### Task 2: Error Types

**Files:**
- Create: `crates/vela-core/src/error.rs`
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Write the test**

Add to `crates/vela-core/src/error.rs`:

```rust
// crates/vela-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VelaError {
    #[error("invalid WASM: {0}")]
    InvalidWasm(String),

    #[error("not a Component Model WASM: {0}")]
    NotComponent(String),

    #[error("WASM parse error: {0}")]
    Wasm(#[from] wasmparser::BinaryReaderError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        let e = VelaError::InvalidWasm("bad magic".into());
        assert_eq!(e.to_string(), "invalid WASM: bad magic");

        let e = VelaError::NotComponent("expected component".into());
        assert_eq!(e.to_string(), "not a Component Model WASM: expected component");

        let e = VelaError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert!(e.to_string().contains("missing"));
    }
}
```

- [ ] **Step 2: Update lib.rs to expose error module**

Replace the contents of `crates/vela-core/src/lib.rs` with:

```rust
// crates/vela-core/src/lib.rs
pub mod error;

pub use error::VelaError;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p vela-core`
Expected: `error_display_messages` passes.

- [ ] **Step 4: Commit**

```bash
git add crates/vela-core/src/error.rs crates/vela-core/src/lib.rs
git commit -m "feat: add VelaError type"
```

---

### Task 3: Component Traversal — Pass-Through Round-Trip

Parse a Component Model WASM and reconstruct it byte-for-byte (no optimization yet). This validates the parse→reconstruct pipeline before adding DCE logic.

**Files:**
- Create: `crates/vela-core/src/component.rs`
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/vela-core/src/component.rs`:

```rust
// crates/vela-core/src/component.rs
use crate::error::VelaError;
use wasmparser::{Encoding, Parser, Payload};
use wasm_encoder::{Component, ModuleArg, RawSection};

/// Parse a Component Model WASM and reconstruct it, applying `process_module`
/// to each core module's bytes. For pass-through, `process_module` returns the
/// module bytes unchanged.
pub fn process_component(
    wasm: &[u8],
    mut process_module: impl FnMut(&[u8]) -> Result<Vec<u8>, VelaError>,
) -> Result<Vec<u8>, VelaError> {
    // Verify this is a Component Model WASM
    let parser = Parser::new(0);
    let mut payloads = parser.parse_all(wasm);

    match payloads.next() {
        Some(Ok(Payload::Version { encoding, .. })) if encoding == Encoding::Component => {}
        _ => {
            return Err(VelaError::NotComponent(
                "input is not a Component Model WASM".into(),
            ));
        }
    }

    // Re-encode the component, replacing core modules with processed versions
    let mut component = Component::new();
    let mut module_depth: u32 = 0;
    let mut module_bytes: Option<Vec<u8>> = None;

    for payload in payloads {
        let payload = payload?;
        match &payload {
            Payload::ModuleSection { parser: _, unchecked_range } => {
                if module_depth == 0 {
                    // Top-level core module: extract bytes, process, and embed
                    let raw = &wasm[unchecked_range.start..unchecked_range.end];
                    let processed = process_module(raw)?;
                    module_bytes = Some(processed);
                }
                module_depth += 1;
            }
            Payload::End { .. } => {
                if module_depth > 0 {
                    module_depth -= 1;
                    if module_depth == 0 {
                        // End of a top-level module — emit the processed module
                        if let Some(bytes) = module_bytes.take() {
                            component.section(&wasm_encoder::ModuleSection(&bytes));
                        }
                        continue;
                    }
                }
            }
            _ => {}
        }

        // Skip payloads that are inside a core module (they're handled as raw bytes)
        if module_depth > 0 {
            continue;
        }

        // For all non-module payloads at component level, re-encode as raw sections
        if let Some((id, range)) = payload.as_section() {
            component.section(&RawSection {
                id,
                data: &wasm[range.start..range.end],
            });
        }
    }

    Ok(component.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::{
        CodeSection, Component as ComponentEncoder, ExportKind, ExportSection, Function,
        FunctionSection, Instruction, Module, TypeSection,
    };

    /// Build a minimal Component Model WASM with one core module containing
    /// a single exported function that returns 42.
    fn build_minimal_component() -> Vec<u8> {
        // Build a core module
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![wasmparser::ValType::I32.into()]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("answer", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();
        let mut f = Function::new(vec![]);
        f.instruction(&Instruction::I32Const(42));
        f.instruction(&Instruction::End);
        code.function(&f);
        module.section(&code);

        let module_bytes = module.finish();

        // Wrap in a component
        let mut component = ComponentEncoder::new();
        component.section(&wasm_encoder::ModuleSection(&module_bytes));
        component.finish()
    }

    #[test]
    fn pass_through_preserves_component() {
        let original = build_minimal_component();
        let result = process_component(&original, |module_bytes| Ok(module_bytes.to_vec()))
            .expect("pass-through should succeed");

        // The result should be valid WASM that wasmparser can parse
        let parser = Parser::new(0);
        let mut found_module = false;
        for payload in parser.parse_all(&result) {
            let payload = payload.expect("result should be valid WASM");
            if matches!(payload, Payload::ModuleSection { .. }) {
                found_module = true;
            }
        }
        assert!(found_module, "result should contain a core module");
    }

    #[test]
    fn rejects_non_component_wasm() {
        // A core module (not a component)
        let module = Module::new().finish();
        let result = process_component(&module, |m| Ok(m.to_vec()));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not a Component Model WASM"),
            "expected NotComponent error, got: {}",
            err
        );
    }
}
```

- [ ] **Step 2: Update lib.rs**

```rust
// crates/vela-core/src/lib.rs
pub mod component;
pub mod error;

pub use error::VelaError;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p vela-core`
Expected: `pass_through_preserves_component` and `rejects_non_component_wasm` both pass.

Note: The implementation is written inline with the test in step 1 because the test and implementation are tightly coupled for this foundational piece. If the tests fail, debug the `process_component` function — the most likely issue is the section re-encoding logic or module depth tracking.

- [ ] **Step 4: Commit**

```bash
git add crates/vela-core/src/component.rs crates/vela-core/src/lib.rs
git commit -m "feat: component traversal with pass-through round-trip"
```

---

### Task 4: Call Graph Construction

**Files:**
- Create: `crates/vela-core/src/callgraph.rs`
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/vela-core/src/callgraph.rs` with the test only (no implementation):

```rust
// crates/vela-core/src/callgraph.rs
use std::collections::{HashMap, HashSet};

use crate::error::VelaError;

/// A call graph mapping each function index to the set of function indices it calls.
#[derive(Debug)]
pub struct CallGraph {
    /// function index → set of called function indices
    pub edges: HashMap<u32, HashSet<u32>>,
    /// Total number of functions (imports + defined)
    pub num_functions: u32,
    /// Number of imported functions (they occupy indices 0..num_imports)
    pub num_imports: u32,
}

impl CallGraph {
    /// Build a call graph from a core module's raw bytes.
    /// Scans each function body for `call` and `call_indirect` instructions.
    pub fn from_module(module_bytes: &[u8]) -> Result<Self, VelaError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::*;

    /// Build a core module with the following call graph:
    /// - func 0: exported "entry", calls func 1
    /// - func 1: calls func 2
    /// - func 2: leaf (no calls)
    /// - func 3: dead (calls func 2, but nobody calls func 3)
    fn build_call_graph_test_module() -> Vec<u8> {
        let mut module = Module::new();

        // One function type: () -> ()
        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        // 4 functions, all type 0
        let mut functions = FunctionSection::new();
        for _ in 0..4 {
            functions.function(0);
        }
        module.section(&functions);

        // Export func 0
        let mut exports = ExportSection::new();
        exports.export("entry", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();

        // func 0: call func 1, return
        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(1));
        f0.instruction(&Instruction::End);
        code.function(&f0);

        // func 1: call func 2, return
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::Call(2));
        f1.instruction(&Instruction::End);
        code.function(&f1);

        // func 2: leaf
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        code.function(&f2);

        // func 3: dead, calls func 2
        let mut f3 = Function::new(vec![]);
        f3.instruction(&Instruction::Call(2));
        f3.instruction(&Instruction::End);
        code.function(&f3);

        module.section(&code);
        module.finish()
    }

    #[test]
    fn builds_call_graph_from_module() {
        let module = build_call_graph_test_module();
        let graph = CallGraph::from_module(&module).expect("should parse module");

        assert_eq!(graph.num_functions, 4);
        assert_eq!(graph.num_imports, 0);

        // func 0 calls func 1
        assert_eq!(graph.edges[&0], HashSet::from([1]));
        // func 1 calls func 2
        assert_eq!(graph.edges[&1], HashSet::from([2]));
        // func 2 calls nothing
        assert_eq!(*graph.edges.get(&2).unwrap_or(&HashSet::new()), HashSet::new());
        // func 3 calls func 2
        assert_eq!(graph.edges[&3], HashSet::from([2]));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vela-core callgraph`
Expected: FAIL with `not yet implemented`

- [ ] **Step 3: Implement `CallGraph::from_module`**

Replace the `todo!()` in `from_module` with:

```rust
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
                    for import in reader {
                        let import = import?;
                        if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                            num_imports += 1;
                        }
                    }
                }
                wasmparser::Payload::FunctionSection(reader) => {
                    num_functions = num_imports + reader.count();
                    // Initialize edges for all functions
                    for i in 0..num_functions {
                        edges.entry(i).or_default();
                    }
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vela-core callgraph`
Expected: PASS

- [ ] **Step 5: Add test for module with imports**

Add to the `tests` module in `callgraph.rs`:

```rust
    /// Build a module with 1 import + 2 defined functions:
    /// - func 0: import "env"."log" (no body)
    /// - func 1: defined, calls func 0 (the import)
    /// - func 2: defined, leaf
    fn build_module_with_import() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import("env", "log", EntityType::Function(0));
        module.section(&imports);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, 1);
        module.section(&exports);

        let mut code = CodeSection::new();

        // func 1 (index 1, first defined): calls func 0 (the import)
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::Call(0));
        f1.instruction(&Instruction::End);
        code.function(&f1);

        // func 2 (index 2, second defined): leaf
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        code.function(&f2);

        module.section(&code);
        module.finish()
    }

    #[test]
    fn handles_imports() {
        let module = build_module_with_import();
        let graph = CallGraph::from_module(&module).expect("should parse");

        assert_eq!(graph.num_functions, 3);
        assert_eq!(graph.num_imports, 1);

        // func 1 calls func 0 (import)
        assert_eq!(graph.edges[&1], HashSet::from([0]));
        // func 2 calls nothing
        assert!(graph.edges.get(&2).unwrap_or(&HashSet::new()).is_empty());
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p vela-core callgraph`
Expected: Both tests pass.

- [ ] **Step 7: Update lib.rs**

```rust
// crates/vela-core/src/lib.rs
pub mod callgraph;
pub mod component;
pub mod error;

pub use error::VelaError;
```

- [ ] **Step 8: Commit**

```bash
git add crates/vela-core/src/callgraph.rs crates/vela-core/src/lib.rs
git commit -m "feat: call graph construction from core module"
```

---

### Task 5: DCE — Root Identification and Reachability

**Files:**
- Create: `crates/vela-core/src/dce.rs`
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/vela-core/src/dce.rs`:

```rust
// crates/vela-core/src/dce.rs
use std::collections::HashSet;

use crate::callgraph::CallGraph;
use crate::error::VelaError;

/// Identify root functions in a core module that must not be eliminated.
/// Returns a set of function indices.
pub fn find_roots(module_bytes: &[u8], graph: &CallGraph) -> Result<HashSet<u32>, VelaError> {
    todo!()
}

/// Given roots and a call graph, return the set of all reachable function indices.
pub fn find_reachable(roots: &HashSet<u32>, graph: &CallGraph) -> HashSet<u32> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::*;

    /// Module with:
    /// - func 0: exported "entry", calls func 1
    /// - func 1: calls func 2
    /// - func 2: leaf
    /// - func 3: dead (calls func 2, but nobody calls func 3)
    fn build_dce_test_module() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        for _ in 0..4 {
            functions.function(0);
        }
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("entry", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::Call(1));
        f0.instruction(&Instruction::End);
        code.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::Call(2));
        f1.instruction(&Instruction::End);
        code.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        code.function(&f2);

        let mut f3 = Function::new(vec![]);
        f3.instruction(&Instruction::Call(2));
        f3.instruction(&Instruction::End);
        code.function(&f3);

        module.section(&code);
        module.finish()
    }

    #[test]
    fn finds_exported_roots() {
        let module = build_dce_test_module();
        let graph = CallGraph::from_module(&module).unwrap();
        let roots = find_roots(&module, &graph).unwrap();

        // func 0 is exported, so it's a root
        assert!(roots.contains(&0));
        // func 3 is not exported, not a root
        assert!(!roots.contains(&3));
    }

    #[test]
    fn finds_reachable_functions() {
        let module = build_dce_test_module();
        let graph = CallGraph::from_module(&module).unwrap();
        let roots = find_roots(&module, &graph).unwrap();
        let reachable = find_reachable(&roots, &graph);

        // func 0, 1, 2 are reachable (0 → 1 → 2)
        assert!(reachable.contains(&0));
        assert!(reachable.contains(&1));
        assert!(reachable.contains(&2));
        // func 3 is NOT reachable
        assert!(!reachable.contains(&3));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vela-core dce`
Expected: FAIL with `not yet implemented`

- [ ] **Step 3: Implement `find_roots`**

Replace the `todo!()` in `find_roots`:

```rust
pub fn find_roots(module_bytes: &[u8], graph: &CallGraph) -> Result<HashSet<u32>, VelaError> {
    let parser = wasmparser::Parser::new(0);
    let mut roots = HashSet::new();

    // All imports are roots (they have no body, occupy index space)
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
                    if let wasmparser::ElementKind::Active { .. }
                    | wasmparser::ElementKind::Declared
                    | wasmparser::ElementKind::Passive = elem.kind
                    {
                        let mut items = elem.items.get_items_reader()?;
                        for _ in 0..items.count() {
                            match items.read()? {
                                wasmparser::ElementItem::Func(idx) => {
                                    roots.insert(idx);
                                }
                                wasmparser::ElementItem::Expr(expr) => {
                                    let mut reader = expr.get_operators_reader();
                                    while !reader.eof() {
                                        if let wasmparser::Operator::RefFunc {
                                            function_index,
                                        } = reader.read()?
                                        {
                                            roots.insert(function_index);
                                        }
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
```

- [ ] **Step 4: Implement `find_reachable`**

Replace the `todo!()` in `find_reachable`:

```rust
pub fn find_reachable(roots: &HashSet<u32>, graph: &CallGraph) -> HashSet<u32> {
    let mut reachable = HashSet::new();
    let mut stack: Vec<u32> = roots.iter().copied().collect();

    while let Some(func) = stack.pop() {
        if !reachable.insert(func) {
            continue; // already visited
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p vela-core dce`
Expected: Both `finds_exported_roots` and `finds_reachable_functions` pass.

- [ ] **Step 6: Add test with elem section and start function**

Add to the `tests` module in `dce.rs`:

```rust
    /// Module with:
    /// - func 0: start function (no export)
    /// - func 1: in elem section (table)
    /// - func 2: dead
    fn build_module_with_start_and_elem() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        for _ in 0..3 {
            functions.function(0);
        }
        module.section(&functions);

        // Table for indirect calls
        let mut tables = TableSection::new();
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            minimum: 1,
            maximum: Some(1),
            table64: false,
            shared: false,
        });
        module.section(&tables);

        // Elem section: func 1 in table
        let mut elements = ElementSection::new();
        elements.active(
            None,
            &ConstExpr::i32_const(0),
            Elements::Functions(&[1]),
        );
        module.section(&elements);

        // Start section: func 0
        module.section(&StartSection { function_index: 0 });

        let mut code = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::End);
        code.function(&f0);

        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::End);
        code.function(&f1);

        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::End);
        code.function(&f2);

        module.section(&code);
        module.finish()
    }

    #[test]
    fn start_and_elem_are_roots() {
        let module = build_module_with_start_and_elem();
        let graph = CallGraph::from_module(&module).unwrap();
        let roots = find_roots(&module, &graph).unwrap();

        assert!(roots.contains(&0), "start function should be a root");
        assert!(roots.contains(&1), "elem function should be a root");
        assert!(!roots.contains(&2), "dead function should not be a root");

        let reachable = find_reachable(&roots, &graph);
        assert!(reachable.contains(&0));
        assert!(reachable.contains(&1));
        assert!(!reachable.contains(&2));
    }
```

- [ ] **Step 7: Run all tests**

Run: `cargo test -p vela-core dce`
Expected: All 3 tests pass.

- [ ] **Step 8: Update lib.rs**

```rust
// crates/vela-core/src/lib.rs
pub mod callgraph;
pub mod component;
pub mod dce;
pub mod error;

pub use error::VelaError;
```

- [ ] **Step 9: Commit**

```bash
git add crates/vela-core/src/dce.rs crates/vela-core/src/lib.rs
git commit -m "feat: DCE root identification and reachability analysis"
```

---

### Task 6: DCE — Dead Function Body Replacement

This is the core of DCE: re-encode a core module, replacing dead function bodies with a single `unreachable` instruction.

**Files:**
- Modify: `crates/vela-core/src/dce.rs`

- [ ] **Step 1: Write the failing test**

Add to `dce.rs` — a public function and test:

```rust
/// Apply DCE to a core module: replace unreachable function bodies with `unreachable`.
/// Returns the optimized module bytes.
pub fn eliminate_dead_code(module_bytes: &[u8]) -> Result<Vec<u8>, VelaError> {
    todo!()
}
```

Add test to the `tests` module:

```rust
    #[test]
    fn eliminates_dead_function_body() {
        let module = build_dce_test_module();
        let original_size = module.len();

        let optimized = eliminate_dead_code(&module).expect("DCE should succeed");

        // Optimized should be smaller (func 3's body replaced with unreachable)
        assert!(
            optimized.len() < original_size,
            "optimized ({}) should be smaller than original ({})",
            optimized.len(),
            original_size
        );

        // Verify it's still valid WASM
        wasmparser::Validator::new().validate_all(&optimized)
            .expect("optimized module should be valid WASM");

        // Verify the call graph: func 3 should have no outgoing calls anymore
        let new_graph = CallGraph::from_module(&optimized).unwrap();
        assert_eq!(new_graph.num_functions, 4, "function count should be preserved");
        assert!(
            new_graph.edges.get(&3).unwrap_or(&HashSet::new()).is_empty(),
            "dead function should have no calls after DCE"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vela-core eliminates_dead`
Expected: FAIL with `not yet implemented`

- [ ] **Step 3: Implement `eliminate_dead_code`**

Replace the `todo!()` with:

```rust
pub fn eliminate_dead_code(module_bytes: &[u8]) -> Result<Vec<u8>, VelaError> {
    let graph = CallGraph::from_module(module_bytes)?;
    let roots = find_roots(module_bytes, &graph)?;
    let reachable = find_reachable(&roots, &graph);

    // Re-encode the module, replacing dead function bodies
    let parser = wasmparser::Parser::new(0);
    let mut module = wasm_encoder::Module::new();
    let mut code_index: u32 = 0;
    let mut in_code_section = false;
    let mut code_section = wasm_encoder::CodeSection::new();

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;
        match &payload {
            wasmparser::Payload::CodeSectionStart { count, .. } => {
                in_code_section = true;
                let _ = count;
                continue;
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let func_index = graph.num_imports + code_index;

                if reachable.contains(&func_index) {
                    // Re-encode the original function body
                    let mut func = wasm_encoder::Function::new(
                        body.get_locals_reader()?
                            .into_iter()
                            .collect::<Result<Vec<_>, _>>()?
                            .into_iter()
                            .map(|(count, ty)| (count, wasm_encoder::ValType::from(ty)))
                            .collect::<Vec<_>>(),
                    );
                    let mut ops = body.get_operators_reader()?;
                    while !ops.eof() {
                        let op = ops.read()?;
                        func.raw(op.raw_bytes(module_bytes));
                    }
                    code_section.function(&func);
                } else {
                    // Dead function: replace with unreachable + end
                    let mut func = wasm_encoder::Function::new(vec![]);
                    func.instruction(&wasm_encoder::Instruction::Unreachable);
                    func.instruction(&wasm_encoder::Instruction::End);
                    code_section.function(&func);
                }

                code_index += 1;
                continue;
            }
            _ => {}
        }

        if in_code_section {
            // We've passed the code section entries — emit accumulated code section
            module.section(&code_section);
            in_code_section = false;
        }

        // Re-encode all other sections as raw
        if let Some((id, range)) = payload.as_section() {
            module.section(&wasm_encoder::RawSection {
                id,
                data: &module_bytes[range.start..range.end],
            });
        }
    }

    // If the code section was the last section
    if in_code_section {
        module.section(&code_section);
    }

    Ok(module.finish())
}
```

Note: The `raw_bytes` approach above may not exist on `Operator`. If it doesn't compile, we need an alternative approach. In that case, replace the re-encoding of live functions with raw byte copying from the original code section entry. Here's the fallback approach for the live function case:

```rust
                if reachable.contains(&func_index) {
                    // Copy the raw function body bytes directly
                    let func = wasm_encoder::RawFunction(&module_bytes[body.range().start..body.range().end]);
                    code_section.function(&func);
                }
```

If neither `raw_bytes` nor `RawFunction` compiles, use wasm-encoder's reencode infrastructure instead:

```rust
                if reachable.contains(&func_index) {
                    use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
                    let mut reencoder = RoundtripReencoder;
                    let func = reencoder.parse_function_body(body)?;
                    code_section.function(&func);
                }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vela-core eliminates_dead`
Expected: PASS

If compilation fails due to API mismatch, try the fallback approaches described in Step 3 in order.

- [ ] **Step 5: Add test that live functions still work**

Add to tests module:

```rust
    #[test]
    fn live_functions_preserved_correctly() {
        // Module: func 0 (exported) returns i32 const 42
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![wasmparser::ValType::I32.into()]);
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0); // func 0: () -> i32
        functions.function(1); // func 1: () -> ()  (dead)
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
        f1.instruction(&Instruction::End);
        code.function(&f1);

        module.section(&code);
        let wasm = module.finish();

        let optimized = eliminate_dead_code(&wasm).unwrap();
        wasmparser::Validator::new().validate_all(&optimized)
            .expect("should be valid WASM");

        // Verify func 0 still has its i32.const 42 instruction
        let parser = wasmparser::Parser::new(0);
        let mut found_const = false;
        for payload in parser.parse_all(&optimized) {
            if let Ok(wasmparser::Payload::CodeSectionEntry(body)) = payload {
                let mut ops = body.get_operators_reader().unwrap();
                while !ops.eof() {
                    if let Ok(wasmparser::Operator::I32Const { value: 42 }) = ops.read() {
                        found_const = true;
                    }
                }
                break; // only check func 0
            }
        }
        assert!(found_const, "exported function should still return 42");
    }
```

- [ ] **Step 6: Run all DCE tests**

Run: `cargo test -p vela-core dce`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/vela-core/src/dce.rs
git commit -m "feat: DCE dead function body replacement"
```

---

### Task 7: Public API — `optimize()`

Wire up the component traversal with DCE to produce the public API.

**Files:**
- Modify: `crates/vela-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Replace `crates/vela-core/src/lib.rs` with:

```rust
// crates/vela-core/src/lib.rs
pub mod callgraph;
pub mod component;
pub mod dce;
pub mod error;

pub use error::VelaError;

/// Configuration for optimization passes.
pub struct OptimizeConfig {
    /// Enable Dead Code Elimination.
    pub dce: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { dce: true }
    }
}

/// Optimize a Component Model WASM binary.
///
/// Applies the enabled optimization passes to each core module within the component.
/// Returns the optimized WASM bytes.
pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    todo!()
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
        types.ty().function(vec![], vec![wasmparser::ValType::I32.into()]);
        types.ty().function(vec![], vec![]);
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0); // func 0: () -> i32, exported
        functions.function(1); // func 1: () -> (), dead
        functions.function(1); // func 2: () -> (), dead
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("answer", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();

        let mut f0 = Function::new(vec![]);
        f0.instruction(&Instruction::I32Const(42));
        f0.instruction(&Instruction::End);
        code.function(&f0);

        // func 1: dead, has a larger body
        let mut f1 = Function::new(vec![]);
        f1.instruction(&Instruction::I32Const(1));
        f1.instruction(&Instruction::I32Const(2));
        f1.instruction(&Instruction::I32Add);
        f1.instruction(&Instruction::Drop);
        f1.instruction(&Instruction::End);
        code.function(&f1);

        // func 2: dead, has a larger body
        let mut f2 = Function::new(vec![]);
        f2.instruction(&Instruction::I32Const(100));
        f2.instruction(&Instruction::I32Const(200));
        f2.instruction(&Instruction::I32Mul);
        f2.instruction(&Instruction::Drop);
        f2.instruction(&Instruction::End);
        code.function(&f2);

        module.section(&code);
        let module_bytes = module.finish();

        let mut component = Component::new();
        component.section(&wasm_encoder::ModuleSection(&module_bytes));
        component.finish()
    }

    #[test]
    fn optimize_reduces_component_size() {
        let original = build_component_with_dead_code();
        let config = OptimizeConfig { dce: true };
        let optimized = optimize(&original, &config).expect("optimize should succeed");

        assert!(
            optimized.len() < original.len(),
            "optimized ({}) should be smaller than original ({})",
            optimized.len(),
            original.len()
        );

        // Should still be valid WASM
        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&optimized) {
            payload.expect("optimized should be parseable");
        }
    }

    #[test]
    fn optimize_with_dce_disabled_passes_through() {
        let original = build_component_with_dead_code();
        let config = OptimizeConfig { dce: false };
        let result = optimize(&original, &config).expect("should succeed");

        // With no optimizations, the output should be a valid component
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vela-core -- --test-threads=1 optimize`
Expected: FAIL with `not yet implemented`

- [ ] **Step 3: Implement `optimize`**

Replace the `todo!()` with:

```rust
pub fn optimize(wasm: &[u8], config: &OptimizeConfig) -> Result<Vec<u8>, VelaError> {
    component::process_component(wasm, |module_bytes| {
        if config.dce {
            dce::eliminate_dead_code(module_bytes)
        } else {
            Ok(module_bytes.to_vec())
        }
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vela-core`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/vela-core/src/lib.rs
git commit -m "feat: public optimize() API wiring component traversal with DCE"
```

---

### Task 8: CLI — `optimize` and `info` Subcommands

**Files:**
- Modify: `crates/vela-cli/src/main.rs`

- [ ] **Step 1: Implement the CLI**

Replace `crates/vela-cli/src/main.rs` with:

```rust
// crates/vela-cli/src/main.rs
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vela", version, about = "WASM Component Model optimizer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Optimize a WASM Component Model binary
    Optimize {
        /// Input .wasm file
        input: PathBuf,

        /// Output .wasm file
        #[arg(short, long)]
        output: PathBuf,

        /// Disable Dead Code Elimination
        #[arg(long)]
        no_dce: bool,
    },
    /// Display information about a WASM Component Model binary
    Info {
        /// Input .wasm file
        input: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Optimize {
            input,
            output,
            no_dce,
        } => {
            let wasm = std::fs::read(&input)?;
            let input_size = wasm.len();

            let config = vela_core::OptimizeConfig { dce: !no_dce };

            let start = std::time::Instant::now();
            let optimized = vela_core::optimize(&wasm, &config)?;
            let elapsed = start.elapsed();

            std::fs::write(&output, &optimized)?;

            let output_size = optimized.len();
            let reduction = input_size - output_size;
            let pct = if input_size > 0 {
                (reduction as f64 / input_size as f64) * 100.0
            } else {
                0.0
            };

            eprintln!(
                "{} -> {} ({} reduced, {:.1}%) in {:.2}s",
                format_size(input_size),
                format_size(output_size),
                format_size(reduction),
                pct,
                elapsed.as_secs_f64(),
            );
        }
        Commands::Info { input } => {
            let wasm = std::fs::read(&input)?;
            print_info(&wasm)?;
        }
    }

    Ok(())
}

fn print_info(wasm: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use wasmparser::{Encoding, Parser, Payload};

    let parser = Parser::new(0);
    let mut module_count = 0;
    let mut total_functions = 0u32;
    let mut total_imports = 0u32;

    for payload in parser.parse_all(wasm) {
        let payload = payload?;
        match payload {
            Payload::Version { encoding, .. } => {
                let kind = match encoding {
                    Encoding::Component => "Component",
                    Encoding::Module => "Module",
                };
                eprintln!("Type: {}", kind);
                eprintln!("Size: {}", format_size(wasm.len()));
            }
            Payload::ModuleSection { .. } => {
                module_count += 1;
            }
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import?;
                    if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        total_imports += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                total_functions += reader.count();
            }
            _ => {}
        }
    }

    eprintln!("Core modules: {}", module_count);
    eprintln!("Functions: {} ({} imported)", total_functions + total_imports, total_imports);

    Ok(())
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}
```

- [ ] **Step 2: Build and test CLI**

Run: `cargo build`
Expected: Compiles successfully.

Run: `cargo run -- --help`
Expected: Shows help text with `optimize` and `info` subcommands.

Run: `cargo run -- optimize --help`
Expected: Shows optimize subcommand help with `--no-dce` and `-o` flags.

- [ ] **Step 3: Commit**

```bash
git add crates/vela-cli/src/main.rs
git commit -m "feat: CLI with optimize and info subcommands"
```

---

### Task 9: Integration Test — Component WASM Round-Trip with wasmtime

Verify that a Component Model WASM with dead code, once optimized, still executes correctly in wasmtime.

**Files:**
- Create: `crates/vela-core/tests/integration_test.rs`
- Modify: `crates/vela-core/Cargo.toml` (add wasmtime features if needed)

- [ ] **Step 1: Check wasmtime component model feature availability**

Run: `cd /Users/mizzy/src/github.com/mizzy/vela && cargo doc -p wasmtime --no-deps 2>&1 | head -20`

If wasmtime's component model requires a feature flag, add it to `crates/vela-core/Cargo.toml`:

```toml
[dev-dependencies]
wasmtime = { version = "43", features = ["component-model"] }
```

- [ ] **Step 2: Write the integration test**

Create `crates/vela-core/tests/integration_test.rs`:

```rust
// crates/vela-core/tests/integration_test.rs
use vela_core::{optimize, OptimizeConfig};

/// Build a Component Model WASM that exports a function callable from wasmtime.
///
/// The component has:
/// - core module with func 0 (exported "answer", returns 42) and func 1 (dead)
/// - component-level type, lift, and export to make it a valid component export
///
/// For simplicity, we use wat (WebAssembly Text) to build a complete valid component
/// rather than stitching it together with wasm-encoder, since component-level
/// canonical lift/export wiring is verbose with the encoder.
fn build_test_component_wat() -> Vec<u8> {
    wat::parse_str(r#"
        (component
            (core module $m
                (func $answer (export "answer") (result i32)
                    i32.const 42
                )
                (func $dead (result i32)
                    i32.const 99
                    i32.const 1
                    i32.add
                )
                (func $also_dead
                    call $dead
                    drop
                )
            )
            (core instance $i (instantiate $m))
            (func (export "answer") (result u32)
                (canon lift (core func $i "answer"))
            )
        )
    "#).expect("WAT should parse")
}

#[test]
fn optimized_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let config = OptimizeConfig { dce: true };
    let optimized = optimize(&original, &config).expect("optimize should succeed");

    // Verify size reduction
    assert!(
        optimized.len() < original.len(),
        "optimized ({}) should be smaller than original ({})",
        optimized.len(),
        original.len()
    );

    // Run in wasmtime
    let mut wasmtime_config = wasmtime::Config::new();
    wasmtime_config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&wasmtime_config).expect("engine");
    let mut store = wasmtime::Store::new(&engine, ());

    let component =
        wasmtime::component::Component::new(&engine, &optimized).expect("should compile");
    let linker: wasmtime::component::Linker<()> = wasmtime::component::Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &component)
        .expect("should instantiate");

    let func = instance
        .get_typed_func::<(), (u32,)>(&mut store, "answer")
        .expect("should find 'answer' export");
    let (result,) = func.call(&mut store, ()).expect("should call");
    assert_eq!(result, 42, "optimized component should return 42");
}

#[test]
fn pass_through_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let config = OptimizeConfig { dce: false };
    let result = optimize(&original, &config).expect("pass-through should succeed");

    let mut wasmtime_config = wasmtime::Config::new();
    wasmtime_config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&wasmtime_config).expect("engine");
    let mut store = wasmtime::Store::new(&engine, ());

    let component = wasmtime::component::Component::new(&engine, &result).expect("should compile");
    let linker: wasmtime::component::Linker<()> = wasmtime::component::Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &component)
        .expect("should instantiate");

    let func = instance
        .get_typed_func::<(), (u32,)>(&mut store, "answer")
        .expect("should find export");
    let (val,) = func.call(&mut store, ()).expect("should call");
    assert_eq!(val, 42);
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test -p vela-core --test integration_test`
Expected: Both tests pass — optimized WASM runs in wasmtime and returns 42.

If compilation fails due to wasmtime feature flags, update `crates/vela-core/Cargo.toml` dev-dependencies accordingly and retry.

- [ ] **Step 4: Commit**

```bash
git add crates/vela-core/tests/integration_test.rs crates/vela-core/Cargo.toml
git commit -m "test: integration test verifying optimized component runs in wasmtime"
```

---

### Task 10: CLI End-to-End Test

Verify the CLI binary works end-to-end with a real file.

**Files:**
- Create: `tests/cli_test.sh`

- [ ] **Step 1: Build the CLI**

Run: `cargo build`
Expected: Compiles, produces `target/debug/vela`.

- [ ] **Step 2: Create a CLI smoke test script**

```bash
#!/usr/bin/env bash
# tests/cli_test.sh — CLI end-to-end smoke test
set -euo pipefail

VELA="cargo run --quiet --"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Generate a test component using wat2wasm via wat crate (use a small Rust helper)
cat > "$TMPDIR/test.wat" <<'WAT'
(component
  (core module $m
    (func (export "f") (result i32) i32.const 1)
    (func (result i32) i32.const 2)
  )
  (core instance $i (instantiate $m))
  (func (export "f") (result u32) (canon lift (core func $i "f")))
)
WAT

# Convert WAT to WASM
cargo run --quiet -p wat-helper -- "$TMPDIR/test.wat" "$TMPDIR/test.wasm" 2>/dev/null || {
    # Fallback: use wat2wasm if available
    if command -v wat2wasm &>/dev/null; then
        wat2wasm "$TMPDIR/test.wat" -o "$TMPDIR/test.wasm"
    else
        echo "SKIP: no wat2wasm available"
        exit 0
    fi
}

# Test optimize
$VELA optimize "$TMPDIR/test.wasm" -o "$TMPDIR/out.wasm"
echo "PASS: optimize produced output"

# Test info
$VELA info "$TMPDIR/test.wasm"
echo "PASS: info ran successfully"

# Verify output exists and is smaller or equal
ORIG=$(wc -c < "$TMPDIR/test.wasm")
OPT=$(wc -c < "$TMPDIR/out.wasm")
echo "Original: ${ORIG}B, Optimized: ${OPT}B"

if [ "$OPT" -le "$ORIG" ]; then
    echo "PASS: output is not larger than input"
else
    echo "FAIL: output is larger"
    exit 1
fi
```

Note: This script depends on having a way to convert WAT to WASM. The integration tests in Task 9 use the `wat` crate in Rust, which is the more reliable approach. This shell script is a secondary smoke test. If `wat2wasm` is not available on the system, the test will skip gracefully.

- [ ] **Step 3: Run the smoke test**

Run: `bash tests/cli_test.sh`
Expected: All assertions pass (or SKIP if wat2wasm unavailable).

- [ ] **Step 4: Commit**

```bash
git add tests/cli_test.sh
git commit -m "test: CLI end-to-end smoke test"
```

---

## Summary

| Task | Description | Key Output |
|------|-------------|------------|
| 1 | Project scaffolding | Workspace compiles |
| 2 | Error types | `VelaError` enum |
| 3 | Component traversal | `process_component()` with pass-through |
| 4 | Call graph | `CallGraph::from_module()` |
| 5 | DCE roots + reachability | `find_roots()`, `find_reachable()` |
| 6 | Dead body replacement | `eliminate_dead_code()` |
| 7 | Public API | `optimize()` |
| 8 | CLI | `vela optimize`, `vela info` |
| 9 | Integration test | wasmtime round-trip verification |
| 10 | CLI smoke test | End-to-end file-based test |
