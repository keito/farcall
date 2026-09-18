use clap::ArgAction;
use clap::Parser;
use std::process;

use crate::machine::DiskType;
use crate::machine::LoadMode;
use crate::machine::Machine;
use crate::machine::MachineStepResult;

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

    let mut machine = Machine::new(&cli.file, load_mode, cli.image_type, cli.verbose)?;

    let exit_code = loop {
        let result = machine.step()?;
        if let MachineStepResult::Exit(code) = result {
            break code;
        }
    };

    Ok(process::ExitCode::from(exit_code))
}
