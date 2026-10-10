use clap::ArgAction;
use clap::Parser;
use std::path::PathBuf;
use std::process;

#[cfg(feature = "terminal")]
use crate::frontend::terminal;
#[cfg(feature = "window")]
use crate::frontend::window;
use crate::machine::DiskImageConfig;
use crate::machine::{BootMode, DiskType, Machine};

mod cpu;
mod frontend;
mod machine;
mod memory;
mod registers;

#[cfg(not(any(feature = "window", feature = "terminal")))]
compile_error!("Enable at least one frontend feature: window or terminal");

#[derive(Clone, clap::ValueEnum)]
enum FrontendChoice {
    #[cfg(feature = "terminal")]
    TerminalGraphics,
    #[cfg(feature = "terminal")]
    TerminalText,
    #[cfg(feature = "window")]
    Window,
}

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

    /// Floppy A image type; defaults to 180k
    #[arg(long, value_enum, requires = "floppy_a", conflicts_with = "com")]
    floppy_a_type: Option<DiskType>,

    /// Image for floppy drive B
    #[arg(long, conflicts_with = "com")]
    floppy_b: Option<PathBuf>,

    /// Floppy B image type; defaults to 180k
    #[arg(long, value_enum, requires = "floppy_b", conflicts_with = "com")]
    floppy_b_type: Option<DiskType>,

    /// Load a BIOS ROM image; turns off BIOS emulation. Requires --floppy-a
    #[arg(long, requires = "floppy_a", conflicts_with = "com")]
    rom: Option<PathBuf>,

    /// Which frontend to use
    #[arg(long, value_enum)]
    frontend: Option<FrontendChoice>,

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
            drive_b: cli.floppy_b.map(|floppy_b| DiskImageConfig {
                path: floppy_b,
                disk_type: cli.floppy_b_type.unwrap_or(DiskType::Floppy180),
            }),
            rom: cli.rom,
        }
    } else {
        BootMode::Com {
            program: cli.com.expect("clap requires --com or --floppy-a"),
        }
    };

    let machine = Machine::new(boot_mode, cli.verbose)?;

    let exit_code = match &cli.frontend {
        #[cfg(feature = "terminal")]
        Some(FrontendChoice::TerminalGraphics) => terminal::run(machine, true)?,

        #[cfg(feature = "terminal")]
        Some(FrontendChoice::TerminalText) => terminal::run(machine, false)?,

        #[cfg(feature = "window")]
        Some(FrontendChoice::Window) => window::run(machine)?,

        // Prioritize window if compiled in and not specified
        #[cfg(feature = "window")]
        None => window::run(machine)?,

        // Fall back to terminal if window is not compiled in
        #[cfg(all(feature = "terminal", not(feature = "window")))]
        None => terminal::run(machine, true)?,
    };

    Ok(exit_code.into())
}
