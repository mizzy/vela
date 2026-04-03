use vela_core::{optimize, OptimizeConfig};

fn build_test_component_wat() -> Vec<u8> {
    wat::parse_str(
        r#"
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
    "#,
    )
    .expect("WAT should parse")
}

fn call_func(wasm: &[u8], name: &str) -> u32 {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&config).expect("engine");
    let mut store = wasmtime::Store::new(&engine, ());

    let component = wasmtime::component::Component::new(&engine, wasm).expect("should compile");
    let linker: wasmtime::component::Linker<()> = wasmtime::component::Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &component)
        .expect("should instantiate");

    let func = instance
        .get_typed_func::<(), (u32,)>(&mut store, name)
        .expect("should find export");
    let (result,) = func.call(&mut store, ()).expect("should call");
    result
}

#[test]
fn optimized_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let optimized = optimize(&original, &OptimizeConfig { dce: true, dfe: true, rume: true }).expect("optimize should succeed");

    assert!(
        optimized.len() < original.len(),
        "optimized ({}) should be smaller than original ({})",
        optimized.len(),
        original.len()
    );

    assert_eq!(call_func(&optimized, "answer"), 42);
}

#[test]
fn pass_through_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let result = optimize(&original, &OptimizeConfig { dce: false, dfe: false, rume: false }).expect("pass-through should succeed");
    assert_eq!(call_func(&result, "answer"), 42);
}

fn build_component_with_duplicates_wat() -> Vec<u8> {
    wat::parse_str(
        r#"
        (component
            (core module $m
                (func $get_a (export "get_a") (result i32)
                    i32.const 42
                )
                (func $get_b (export "get_b") (result i32)
                    i32.const 42
                )
                (func $dead (result i32)
                    i32.const 99
                )
            )
            (core instance $i (instantiate $m))
            (func (export "get-a") (result u32)
                (canon lift (core func $i "get_a"))
            )
            (func (export "get-b") (result u32)
                (canon lift (core func $i "get_b"))
            )
        )
    "#,
    )
    .expect("WAT should parse")
}

#[test]
fn dfe_merges_duplicates_and_runs_correctly() {
    let original = build_component_with_duplicates_wat();
    let optimized = optimize(&original, &OptimizeConfig { dce: true, dfe: true, rume: true })
        .expect("optimize should succeed");

    assert!(optimized.len() < original.len());

    // Both exports should still work correctly
    assert_eq!(call_func(&optimized, "get-a"), 42);
    assert_eq!(call_func(&optimized, "get-b"), 42);
}

fn build_component_with_unused_global_wat() -> Vec<u8> {
    wat::parse_str(
        r#"
        (component
            (core module $m
                (global $used (mut i32) (i32.const 0))
                (global $unused (mut i32) (i32.const 999))
                (func $get (export "get") (result i32)
                    global.get $used
                )
                (func $set (export "set") (param i32)
                    local.get 0
                    global.set $used
                )
            )
            (core instance $i (instantiate $m))
            (func (export "get") (result u32)
                (canon lift (core func $i "get"))
            )
            (func (export "set") (param "x" u32)
                (canon lift (core func $i "set"))
            )
        )
    "#,
    )
    .expect("WAT should parse")
}

#[test]
fn rume_removes_unused_global_and_runs() {
    let original = build_component_with_unused_global_wat();
    let optimized = optimize(
        &original,
        &OptimizeConfig { dce: true, dfe: true, rume: true },
    )
    .expect("optimize should succeed");

    assert!(optimized.len() < original.len());

    // Verify the optimized component still works
    assert_eq!(call_func(&optimized, "get"), 0);
}
