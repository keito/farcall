use std::{
    io::{Write, stdout},
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, KeyCode, KeyModifiers},
    terminal::SetTitle,
};
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
mod text;

const FRAME_INTERVAL: Duration = Duration::from_micros(33_333); // 30 Hz
const EXECUTION_BUDGET: Duration = Duration::from_millis(10);
const CPU_HZ: u64 = 4_772_727;

const PADDING: usize = 8;

pub(crate) fn run(mut machine: Machine, use_kitty_graphics: bool) -> anyhow::Result<u8> {
    let guard = TerminalGuard::new(use_kitty_graphics)?;

    // Set title
    crossterm::execute!(stdout(), SetTitle("pc86 - Press Ctrl-] to exit"))?;

    let mut current_frame = [0u8; 640 * 200 * 3];
    let mut previous_frame = [0u8; 640 * 200 * 3];
    let mut previous_size: (usize, usize) = (0, 0);

    // Move cursor to row 1, column 1
    stdout().write_all(b"\x1b[H")?;

    let mut exit_code: Option<u8> = None;
    let mut next_frame = Instant::now();

    let mut pacing_start = Instant::now();
    let mut starting_cycles = machine.total_elapsed_cycles();

    // Render each frame, at FRAME_INTERVAL
    loop {
        let frame_start = Instant::now();
        let execution_deadline = frame_start + EXECUTION_BUDGET;

        // Poll input
        for _ in 0..16 {
            if !event::poll(Duration::ZERO)? {
                break;
            }
            let event = event::read()?;
            if let event::Event::Key(key_event) = event {
                // Handle Ctrl-]
                if key_event.code == KeyCode::Char(']')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && key_event.is_press()
                {
                    return Ok(0);
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
        }

        let target_cycles = pacing_start.elapsed().as_nanos() * u128::from(CPU_HZ) / 1_000_000_000;

        // Step until exit, guest has caught up with real time, or execution deadline is reached
        while u128::from(machine.total_elapsed_cycles() - starting_cycles) < target_cycles
            && Instant::now() < execution_deadline
        {
            let result = machine.step()?;
            match result {
                MachineStepResult::Continue => (),
                MachineStepResult::WaitingForInput => {
                    pacing_start = Instant::now();
                    starting_cycles = machine.total_elapsed_cycles();
                    break;
                }
                MachineStepResult::Exit(code) => {
                    exit_code = Some(code);
                    break;
                }
            }
        }

        // Is the next frame due? (or is this our last frame)
        if Instant::now() >= next_frame || exit_code.is_some() {
            next_frame = Instant::now() + FRAME_INTERVAL;

            if use_kitty_graphics {
                machine.render_video(&mut current_frame);

                let winsize = tcgetwinsize(&guard.terminal)?;
                let (display_width, display_height) = calc_display_size(winsize);

                // Send new frame if different from previous frame
                if current_frame != previous_frame
                    || previous_size != (display_width, display_height)
                {
                    if display_width == 0 || display_height == 0 {
                        anyhow::bail!("Terminal is too small");
                    }

                    let data = graphics::encode_image(
                        &current_frame,
                        640,
                        200,
                        display_width,
                        display_height,
                    )?;
                    for line in data {
                        stdout().write_all(&line.as_bytes())?;
                    }
                    stdout().flush()?;

                    std::mem::swap(&mut current_frame, &mut previous_frame);
                    previous_size = (display_width, display_height);
                }
            } else {
                let screen = machine.render_text_screen();
                // TODO: Don't send frame if same as previous frame
                if let Some(screen) = screen {
                    let mut buf: Vec<u8> = vec![];
                    text::render(&screen, &mut buf)?;
                    stdout().write_all(&buf)?;
                    stdout().flush()?;
                } else {
                    stdout().write_all(b"\x1b[2J\x1b[H")?;
                    stdout().write_all(b"The video card is rendering graphics, which is not supported in the Text \r\nTerminal frontend. Press Ctrl-] to terminate.")?;
                    stdout().flush()?;
                }
            }
        }

        if let Some(code) = exit_code {
            return Ok(code);
        }

        std::thread::sleep(Duration::from_millis(1));
    }
}

fn calc_display_size(winsize: Winsize) -> (usize, usize) {
    // Reserve some margin
    let width_px = f64::from(winsize.ws_xpixel.saturating_sub(PADDING as u16 * 2));
    let height_px = f64::from(winsize.ws_ypixel.saturating_sub(PADDING as u16 * 2));

    let image_width = width_px.min(height_px * (4.0 / 3.0));
    let image_height = image_width * (3.0 / 4.0);

    (image_width as usize, image_height as usize)
}
