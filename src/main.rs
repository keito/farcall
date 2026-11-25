use clap::ArgAction;
use clap::Parser;
use std::path::PathBuf;
use std::process;
#[cfg(feature = "terminal")]
use std::time::Duration;

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

    /// Write-protect floppy drive A
    #[arg(long, requires = "floppy_a", conflicts_with = "com")]
    floppy_a_read_only: bool,

    /// Image for floppy drive B
    #[arg(long, conflicts_with = "com")]
    floppy_b: Option<PathBuf>,

    /// Floppy B image type; defaults to 180k
    #[arg(long, value_enum, requires = "floppy_b", conflicts_with = "com")]
    floppy_b_type: Option<DiskType>,

    /// Image for a hard disk drive
    #[arg(long, requires = "rom_fixed_disk", conflicts_with = "com")]
    hdd: Option<PathBuf>,

    /// Load a BIOS ROM image; turns off BIOS emulation. Requires --floppy-a
    #[arg(long, requires = "floppy_a", conflicts_with = "com")]
    rom: Option<PathBuf>,

    /// Load a fixed-disk option ROM. Requires --rom
    #[arg(long, requires = "rom", conflicts_with = "com")]
    rom_fixed_disk: Option<PathBuf>,

    /// Which frontend to use
    #[arg(long, value_enum)]
    frontend: Option<FrontendChoice>,

    /// Write floppy drive B's resulting image to this path on exit
    #[arg(long, requires = "floppy_b", conflicts_with = "com")]
    write_floppy_b: Option<PathBuf>,

    /// Terminate emulator after IDLE_TIMEOUT seconds with no keyboard input (terminal frontend only)
    #[arg(long)]
    #[cfg(feature = "terminal")]
    idle_timeout: Option<u64>,

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
                write_protected: cli.floppy_a_read_only,
            },
            drive_b: cli.floppy_b.map(|floppy_b| DiskImageConfig {
                path: floppy_b,
                disk_type: cli.floppy_b_type.unwrap_or(DiskType::Floppy180),
                write_protected: false,
            }),
            hdd: cli.hdd,
            rom: cli.rom,
            rom_fixed_disk: cli.rom_fixed_disk,
        }
    } else {
        BootMode::Com {
            program: cli.com.expect("clap requires --com or --floppy-a"),
        }
    };

    #[cfg(feature = "terminal")]
    let idle_timeout = cli.idle_timeout.map(|s| Duration::from_secs(s));

    let mut machine = Machine::new(boot_mode, cli.verbose)?;

    let result = match &cli.frontend {
        #[cfg(feature = "terminal")]
        Some(FrontendChoice::TerminalGraphics) => terminal::run(&mut machine, true, idle_timeout),

        #[cfg(feature = "terminal")]
        Some(FrontendChoice::TerminalText) => terminal::run(&mut machine, false, idle_timeout),

        #[cfg(feature = "window")]
        Some(FrontendChoice::Window) => window::run(&mut machine),

        // Prioritize window if compiled in and not specified
        #[cfg(feature = "window")]
        None => window::run(&mut machine),

        // Fall back to terminal if window is not compiled in
        #[cfg(all(feature = "terminal", not(feature = "window")))]
        None => terminal::run(&mut machine, true, idle_timeout),
    };

    if let Some(write_floppy_b_path) = cli.write_floppy_b
        && let Some(floppy_b_image) = machine.floppy_image(1)
    {
        std::fs::write(write_floppy_b_path, floppy_b_image)?;
    }

    let exit_code = result?;
    Ok(exit_code.into())
}
