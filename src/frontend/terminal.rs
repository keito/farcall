use std::io::Write;

use crate::machine::Machine;

mod graphics;

pub(crate) fn run(machine: Machine) -> anyhow::Result<u8> {
    let data = graphics::image_testings();
    for line in data {
        std::io::stdout().write_all(&line.as_bytes())?;
    }

    std::io::stdout().flush()?;

    Ok(0)
}
