use clap::Parser;
use std::process;

use crate::machine::LoadMode;

mod cpu;
mod machine;
mod memory;
mod registers;

#[derive(Parser)]
struct Cli {
    /// COM file or boot sector image to load
    file: String,

    /// Treat FILE as a boot sector image
    #[arg(long)]
    boot: bool,
}

fn main() {
    let cli = Cli::parse();
    let load_mode = if cli.boot {
        LoadMode::Boot
    } else {
        LoadMode::Com
    };
    match machine::main_loop(&cli.file, load_mode) {
        Ok(code) => {
            eprintln!("Exit code {}", code);
            process::exit(code as i32);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(-1);
        }
    }
}
