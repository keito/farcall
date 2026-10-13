use std::{
    io::{Write, stdout},
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, KeyCode, KeyEvent, KeyModifiers},
    terminal::SetTitle,
};
use rustix::termios::{Winsize, tcgetwinsize};

use crate::{
    frontend::terminal::keyboard::{
        crossterm_to_bios_keystroke, crossterm_to_pc_scan_code, normalize_basic_crossterm_key_event,
    },
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

enum Renderer {
    Graphics(graphics::Renderer),
    Text(text::Renderer),
}

impl Renderer {
    pub(crate) fn render(
        &mut self,
        machine: &Machine,
        winsize: Winsize,
        stdout: &mut impl Write,
    ) -> anyhow::Result<()> {
        match self {
            Renderer::Graphics(graphics) => graphics.render(machine, winsize, stdout),
            Renderer::Text(text) => text.render(machine, winsize, stdout),
        }
    }

    pub(crate) fn invalidate(&mut self) {
        match self {
            Renderer::Graphics(_) => (),
            Renderer::Text(text) => text.invalidate(),
        }
    }
}

fn handle_enhanced_key_event(machine: &mut Machine, key_event: KeyEvent) -> bool {
    // Handle Ctrl-]
    if key_event.code == KeyCode::Char(']')
        && key_event.modifiers.contains(KeyModifiers::CONTROL)
        && key_event.is_press()
    {
        return true;
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

    false
}

fn handle_basic_key_event(machine: &mut Machine, key_event: KeyEvent) -> bool {
    // Handle Ctrl-]
    if key_event.code == KeyCode::Char('5') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    // Convert shifted characters to shift + unshifted character
    let key_event = normalize_basic_crossterm_key_event(key_event);

    match machine.keyboard_input_mode() {
        BiosKeystrokes => {
            if let Some(keystroke) = crossterm_to_bios_keystroke(&key_event) {
                machine.enqueue_bios_keystroke(keystroke);
            }
        }
        PcHardware => {
            if let Some(scan_code) = crossterm_to_pc_scan_code(&key_event) {
                let modifiers = [
                    (KeyModifiers::SHIFT, 0x2A),
                    (KeyModifiers::CONTROL, 0x1D),
                    (KeyModifiers::ALT, 0x38),
                ];

                // Simulate modifier key key press
                for &(modifier, code) in &modifiers {
                    if key_event.modifiers.contains(modifier) {
                        machine.enqueue_pc_keyboard_event(code, true);
                    }
                }

                // Simulate key press
                machine.enqueue_pc_keyboard_event(scan_code, true);
                machine.enqueue_pc_keyboard_event(scan_code, false);

                // Simulate modifier key release
                for &(modifier, code) in modifiers.iter().rev() {
                    if key_event.modifiers.contains(modifier) {
                        machine.enqueue_pc_keyboard_event(code, false);
                    }
                }
            }
        }
    }

    false
}

pub(crate) fn run(machine: &mut Machine, use_graphics: bool) -> anyhow::Result<u8> {
    let guard = TerminalGuard::new(use_graphics)?;

    // Set title
    crossterm::execute!(stdout(), SetTitle("Farcall - Press Ctrl-] to exit"))?;

    // Move cursor to row 1, column 1
    stdout().write_all(b"\x1b[H")?;

    let mut exit_code: Option<u8> = None;
    let mut next_frame = Instant::now();

    let mut pacing_start = Instant::now();
    let mut starting_cycles = machine.total_elapsed_cycles();

    let mut renderer: Renderer = if use_graphics {
        Renderer::Graphics(graphics::Renderer::new())
    } else {
        Renderer::Text(text::Renderer::new())
    };

    // Render each frame, at FRAME_INTERVAL
    loop {
        let frame_start = Instant::now();
        let execution_deadline = frame_start + EXECUTION_BUDGET;

        // Poll input
        for _ in 0..16 {
            if !event::poll(Duration::ZERO)? {
                break;
            }
            match event::read()? {
                event::Event::Key(key_event) => {
                    if guard.keyboard_enhancement_enabled() {
                        if handle_enhanced_key_event(machine, key_event) {
                            return Ok(0);
                        }
                    } else {
                        if handle_basic_key_event(machine, key_event) {
                            return Ok(0);
                        }
                    }
                }
                event::Event::Resize(_, _) => {
                    renderer.invalidate();
                }
                _ => (),
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

            let winsize = tcgetwinsize(&guard.terminal)?;
            renderer.render(&machine, winsize, &mut stdout())?;
        }

        if let Some(code) = exit_code {
            return Ok(code);
        }

        std::thread::sleep(Duration::from_millis(1));
    }
}
