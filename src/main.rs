use clap::ArgAction;
use clap::Parser;
use std::process;

use crate::frontend::window;
use crate::machine::DiskType;
use crate::machine::LoadMode;

mod cpu;
mod frontend;
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

    /// How much verbose debug output to print
    #[arg(short, long, default_value = "0", action = ArgAction::Count)]
    verbose: u8,
}

fn main() -> anyhow::Result<process::ExitCode> {
    let cli = Cli::parse();
    let load_mode = if cli.boot {
        LoadMode::Boot
    } else {
        LoadMode::Com
    };

    let exit_code = window::run(&cli.file, load_mode, cli.image_type, cli.verbose)?;
    Ok(exit_code.into())
}
