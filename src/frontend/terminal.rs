use std::{
    fs::File,
    io::{Write, stdout},
    time::{Duration, Instant},
};

use rustix::termios::{Winsize, tcgetwinsize};

use crate::machine::{Machine, MachineStepResult};

mod graphics;

const FRAME_INTERVAL: Duration = Duration::from_micros(66_667); // 15 Hz
const EXECUTION_BUDGET: Duration = Duration::from_millis(2);

pub(crate) fn run(machine: &mut Machine) -> anyhow::Result<u8> {
    let mut frame = [0u8; 640 * 200 * 3];

    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return Err(anyhow::anyhow!(
            "This program requires an interactive terminal."
        ));
    };

    // Move cursor to row 1, column 1
    stdout().write_all(b"\x1b[H")?;

    let mut exit_code: Option<u8> = None;

    // Render each frame, at FRAME_INTERVAL
    loop {
        let frame_start = Instant::now();
        let execution_deadline = frame_start + EXECUTION_BUDGET;

        // Step until exit or execution deadline is reached
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
                    exit_code = Some(code);
                    break;
                }
            }

            if execution_deadline < Instant::now() {
                break;
            }
        }

        render_frame(machine, &mut frame, &tty)?;

        if let Some(code) = exit_code {
            return Ok(code);
        }

        let remaining = (frame_start + FRAME_INTERVAL).saturating_duration_since(Instant::now());
        std::thread::sleep(remaining);
    }
}

fn render_frame(machine: &Machine, frame: &mut [u8], tty: &File) -> anyhow::Result<()> {
    machine.render_video(frame);

    let winsize = tcgetwinsize(&tty)?;
    let (display_columns, display_rows) = calc_display_size(winsize);

    let data = graphics::encode_image(&frame, 640, 200, display_columns, display_rows);
    for line in data {
        stdout().write_all(&line.as_bytes())?;
    }

    stdout().flush()?;

    Ok(())
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
