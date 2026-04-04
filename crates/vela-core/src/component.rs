// crates/vela-core/src/component.rs
use crate::error::VelaError;
use wasmparser::{Encoding, Parser, Payload};
use wasm_encoder::{Component, ComponentSectionId, RawSection};

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

    // Re-encode the component, replacing core modules with processed versions.
    // Nested components and modules are tracked by depth so their internal
    // payloads are not accidentally flattened into the top-level component.
    let mut component = Component::new();
    let mut nested_depth: u32 = 0;
    let mut module_bytes: Option<Vec<u8>> = None;
    let mut nested_component_range: Option<std::ops::Range<usize>> = None;

    for payload in payloads {
        let payload = payload?;
        match &payload {
            Payload::ModuleSection { parser: _, unchecked_range } => {
                if nested_depth == 0 {
                    let raw = &wasm[unchecked_range.start..unchecked_range.end];
                    let processed = process_module(raw)?;
                    module_bytes = Some(processed);
                }
                nested_depth += 1;
            }
            Payload::ComponentSection { unchecked_range, .. } => {
                if nested_depth == 0 {
                    // Save the range of the nested component to emit as raw bytes
                    nested_component_range = Some(unchecked_range.start..unchecked_range.end);
                }
                nested_depth += 1;
            }
            Payload::End { .. } => {
                if nested_depth > 0 {
                    nested_depth -= 1;
                    if nested_depth == 0 {
                        if let Some(bytes) = module_bytes.take() {
                            component.section(&RawSection {
                                id: ComponentSectionId::CoreModule.into(),
                                data: &bytes,
                            });
                        } else if let Some(range) = nested_component_range.take() {
                            component.section(&RawSection {
                                id: ComponentSectionId::Component.into(),
                                data: &wasm[range],
                            });
                        }
                        continue;
                    }
                }
            }
            _ => {}
        }

        if nested_depth > 0 {
            continue;
        }

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

    fn build_minimal_component() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![wasm_encoder::ValType::I32]);
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

        let mut component = ComponentEncoder::new();
        component.section(&wasm_encoder::ModuleSection(&module));
        component.finish()
    }

    #[test]
    fn pass_through_preserves_component() {
        let original = build_minimal_component();
        let result = process_component(&original, |module_bytes| Ok(module_bytes.to_vec()))
            .expect("pass-through should succeed");

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
