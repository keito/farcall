use std::{
    io::{Write, stdout},
    time::{Duration, Instant},
};

use crossterm::event::{self, KeyCode, KeyModifiers};
use rustix::termios::{Winsize, tcgetwinsize};

use crate::{
    frontend::terminal::keyboard::{crossterm_to_bios_keystroke, crossterm_to_pc_scan_code},
    machine::{
        KeyboardInputMode::{BiosKeystrokes, PcHardware},
        Machine, MachineStepResult,
    },
};
use guard::TerminalGuard;

mod graphics;
mod guard;
mod keyboard;

const FRAME_INTERVAL: Duration = Duration::from_micros(66_667); // 15 Hz
const EXECUTION_BUDGET: Duration = Duration::from_millis(2);

pub(crate) fn run(machine: &mut Machine) -> anyhow::Result<u8> {
    let guard = TerminalGuard::new()?;

    let mut current_frame = [0u8; 640 * 200 * 3];
    let mut previous_frame = [0u8; 640 * 200 * 3];
    let mut previous_size: (usize, usize) = (0, 0);

    // Move cursor to row 1, column 1
    stdout().write_all(b"\x1b[H")?;

    let mut exit_code: Option<u8> = None;

    // Render each frame, at FRAME_INTERVAL
    loop {
        let frame_start = Instant::now();
        let execution_deadline = frame_start + EXECUTION_BUDGET;

        for _ in 0..16 {
            if event::poll(Duration::ZERO)? {
                let event = event::read()?;
                if let event::Event::Key(key_event) = event {
                    // Handle Ctrl-C
                    if key_event.code == KeyCode::Char('c')
                        && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && key_event.is_press()
                    {
                        return Ok(255);
                    }

                    match machine.keyboard_input_mode() {
                        BiosKeystrokes => {
                            if let Some(keystroke) = crossterm_to_bios_keystroke(&key_event) {
                                machine.enqueue_bios_keystroke(keystroke);
                            }
                        }
                        PcHardware => {
                            if let Some(scan_code) = crossterm_to_pc_scan_code(&key_event) {
                                machine.enqueue_pc_keyboard_event(
                                    scan_code,
                                    key_event.is_press() || key_event.is_repeat(),
                                );
                            }
                        }
                    }
                }
            } else {
                break;
            }
        }

        // Step until exit or execution deadline is reached
        loop {
            let result = machine.step()?;
            match result {
                MachineStepResult::Continue => (),
                MachineStepResult::WaitingForInput => break,
                MachineStepResult::Exit(code) => {
                    exit_code = Some(code);
                    break;
                }
            }

            if execution_deadline < Instant::now() {
                break;
            }
        }

        machine.render_video(&mut current_frame);

        let winsize = tcgetwinsize(&guard.terminal)?;
        let (display_columns, display_rows) = calc_display_size(winsize);

        // Send new frame if different from previous frame
        if current_frame != previous_frame || previous_size != (display_columns, display_rows) {
            let data =
                graphics::encode_image(&current_frame, 640, 200, display_columns, display_rows);
            for line in data {
                stdout().write_all(&line.as_bytes())?;
            }
            stdout().flush()?;

            std::mem::swap(&mut current_frame, &mut previous_frame);
            previous_size = (display_columns, display_rows);
        }

        if let Some(code) = exit_code {
            return Ok(code);
        }

        let remaining = (frame_start + FRAME_INTERVAL).saturating_duration_since(Instant::now());
        std::thread::sleep(remaining);
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
