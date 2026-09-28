use std::io::{Write, stdout};

use rustix::termios::{Winsize, tcgetwinsize};

use crate::machine::{Machine, MachineStepResult};

mod graphics;

pub(crate) fn run(machine: &mut Machine) -> anyhow::Result<u8> {
    let mut frame = [0u8; 640 * 200 * 3];

    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return Err(anyhow::anyhow!(
            "This program requires an interactive terminal."
        ));
    };

    // Move cursor to row 1, column 1
    stdout().write_all(b"\x1b[H")?;

    loop {
        let result = machine.step()?;
        match result {
            MachineStepResult::Continue => {}
            MachineStepResult::WaitingForInput => {
                return Err(anyhow::anyhow!(
                    "Waiting for input is not supported in terminal mode"
                ));
            }
            MachineStepResult::Exit(code) => {
                return Ok(code);
            }
        }

        machine.render_video(&mut frame);

        let winsize = tcgetwinsize(&tty)?;
        let (display_columns, display_rows) = calc_display_size(winsize);

        let data = graphics::encode_image(&frame, 640, 200, display_columns, display_rows);
        for line in data {
            stdout().write_all(&line.as_bytes())?;
        }

        stdout().flush()?;
    }
}

fn calc_display_size(winsize: Winsize) -> (usize, usize) {
    let columns = f64::from(winsize.ws_col);
    let rows = f64::from(winsize.ws_row);
    let width_px = f64::from(winsize.ws_xpixel);
    let height_px = f64::from(winsize.ws_ypixel);

    let cell_width = width_px / columns;
    let cell_height = height_px / rows;

    let image_width = width_px.min(height_px * (4.0 / 3.0));
    let image_height = image_width * (3.0 / 4.0);

    let columns = (image_width / cell_width) as usize;
    let rows = (image_height / cell_height) as usize;

    (columns, rows)
}
