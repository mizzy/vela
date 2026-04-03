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
        Commands::Optimize { input, output, no_dce } => {
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
                for import in reader.into_imports() {
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
