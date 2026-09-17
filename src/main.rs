use clap::Parser;
use std::process;

use crate::machine::DiskType;
use crate::machine::LoadMode;

mod cpu;
mod machine;
mod memory;
mod registers;

#[derive(Parser)]
struct Cli {
    /// COM file or boot sector image to load
    file: String,

    /// Treat FILE as a boot sector image, and turn off DOS emulation
    #[arg(long)]
    boot: bool,

    /// Boot sector image type
    #[arg(long, value_enum)]
    image_type: Option<DiskType>,
}

fn main() {
    let cli = Cli::parse();
    let load_mode = if cli.boot {
        LoadMode::Boot
    } else {
        LoadMode::Com
    };
    match machine::main_loop(&cli.file, load_mode, cli.image_type) {
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
