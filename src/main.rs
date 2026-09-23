use clap::ArgAction;
use clap::Parser;
use std::path::PathBuf;
use std::process;

use crate::frontend::window;
use crate::machine::DiskImageConfig;
use crate::machine::{BootMode, DiskType, Machine};

mod cpu;
mod frontend;
mod machine;
mod memory;
mod registers;

#[derive(Parser)]
#[command(group(
    clap::ArgGroup::new("input")
        .required(true)
        .multiple(false)
        .args(["com", "floppy_a"])
))]
struct Cli {
    /// COM file to run with DOS + BIOS emulation
    #[arg(long)]
    com: Option<PathBuf>,

    /// Image for floppy drive A to boot from; turns off DOS emulation
    #[arg(long)]
    floppy_a: Option<PathBuf>,

    /// Load a BIOS ROM image; turns off BIOS emulation. Requires --floppy-a
    #[arg(long, requires = "floppy_a", conflicts_with = "com")]
    rom: Option<PathBuf>,

    /// Floppy A image type; defaults to 180k
    #[arg(long, value_enum, requires = "floppy_a", conflicts_with = "com")]
    floppy_a_type: Option<DiskType>,

    /// How much verbose debug output to print
    #[arg(short, long, default_value = "0", action = ArgAction::Count)]
    verbose: u8,
}

fn main() -> anyhow::Result<process::ExitCode> {
    let cli = Cli::parse();

    let boot_mode = if let Some(floppy_a) = cli.floppy_a {
        BootMode::Floppy {
            drive_a: DiskImageConfig {
                path: floppy_a,
                disk_type: cli.floppy_a_type.unwrap_or(DiskType::Floppy180),
            },
            drive_b: None,
            rom: cli.rom,
        }
    } else {
        BootMode::Com {
            program: cli.com.expect("clap requires --com or --floppy-a"),
        }
    };

    let machine = Machine::new(boot_mode, cli.verbose)?;
    let exit_code = window::run(machine)?;

    Ok(exit_code.into())
}
