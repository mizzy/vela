fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: gen_test_wasm <output_path>");
        std::process::exit(1);
    }

    let output_path = &args[1];

    // Parse component model from WAT source
    let wasm = wat::parse_str(
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
            )
            (core instance $i (instantiate $m))
            (func (export "answer") (result u32)
                (canon lift (core func $i "answer"))
            )
        )
        "#,
    )
    .expect("WAT should parse");

    std::fs::write(output_path, wasm).expect("failed to write WASM file");
}
