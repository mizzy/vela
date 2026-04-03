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

#[test]
fn optimized_component_runs_in_wasmtime() {
    let original = build_test_component_wat();
    let config = OptimizeConfig { dce: true };
    let optimized = optimize(&original, &config).expect("optimize should succeed");

    assert!(
        optimized.len() < original.len(),
        "optimized ({}) should be smaller than original ({})",
        optimized.len(),
        original.len()
    );

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
