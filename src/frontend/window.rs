use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use pixels::{Pixels, SurfaceTexture};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

use crate::{
    frontend::palette_color_to_rgb,
    machine::{KeyboardInputMode, Machine, MachineStepResult},
};
use cga_renderer::CgaRenderer;
use keyboard::{ModifierState, winit_to_bios_key_stroke, winit_to_pc_scan_code};

mod cga_renderer;
mod keyboard;

pub struct App<'a> {
    cga_renderer: Option<CgaRenderer>,
    error: Option<anyhow::Error>,
    exit_code: Option<u8>,
    machine: &'a mut Machine,
    modifier_state: ModifierState,
    next_frame: Instant,
    pixels: Option<Pixels<'static>>,
    indexed_frame: Box<[u8]>,
    window: Option<Arc<Window>>,
    pacing_start: Instant,
    starting_cycles: u64,
}

const WIDTH: u32 = 640;
const HEIGHT: u32 = 200;
const CORRECTED_HEIGHT: u32 = 480;
const WINDOW_TITLE: &str = "Farcall";

const FRAME_INTERVAL: Duration = Duration::from_micros(16_700); // 60 Hz
const EXECUTION_BUDGET: Duration = Duration::from_millis(10);
const CPU_HZ: u64 = 4_772_727;

impl App<'_> {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| -> anyhow::Result<_> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title(WINDOW_TITLE)
                        .with_inner_size(LogicalSize::new(WIDTH, CORRECTED_HEIGHT))
                        .with_min_inner_size(LogicalSize::new(WIDTH, CORRECTED_HEIGHT)),
                )?,
            );
            let size = window.inner_size();
            let surface = SurfaceTexture::new(size.width, size.height, window.clone());
            let pixels = Pixels::new(WIDTH, HEIGHT, surface)?;
            self.cga_renderer = Some(CgaRenderer::new(&pixels));
            Ok((window, pixels))
        })();
        match result {
            Ok((window, pixels)) => {
                self.window = Some(window.clone());
                window.set_title(WINDOW_TITLE);
                self.pixels = Some(pixels);
                self.pacing_start = Instant::now();
                self.starting_cycles = self.machine.total_elapsed_cycles();
            }
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if window.id() != id {
            return;
        }
        match event {
            WindowEvent::Resized(size) => {
                let pixels = self.pixels.as_mut().unwrap();
                // Minimized windows can report zero
                if size.width != 0
                    && size.height != 0
                    && let Err(error) = pixels.resize_surface(size.width, size.height)
                {
                    self.fail(event_loop, error.into());
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                if size.width == 0 || size.height == 0 {
                    return;
                }
                let deadline = Instant::now() + EXECUTION_BUDGET;

                let target_cycles =
                    self.pacing_start.elapsed().as_nanos() * u128::from(CPU_HZ) / 1_000_000_000;

                // Step until exit, guest has caught up with real time, or execution deadline is reached
                while self.exit_code.is_none()
                    && u128::from(self.machine.total_elapsed_cycles() - self.starting_cycles)
                        < target_cycles
                    && Instant::now() < deadline
                {
                    match self.machine.step() {
                        Ok(MachineStepResult::Continue) => {}
                        Ok(MachineStepResult::WaitingForInput) => {
                            self.pacing_start = Instant::now();
                            self.starting_cycles = self.machine.total_elapsed_cycles();
                            break;
                        }
                        Ok(MachineStepResult::Exit(exit_code)) => {
                            self.exit_code = Some(exit_code);
                            break;
                        }
                        Err(error) => {
                            self.fail(event_loop, error.into());
                            return;
                        }
                    }
                }
                window.pre_present_notify();
                let pixels = self.pixels.as_mut().unwrap();

                // Render temporarily into memory and convert indexed colors to RGBA
                self.machine.render_video(&mut self.indexed_frame);
                for (color, rgba) in self
                    .indexed_frame
                    .iter()
                    .zip(pixels.frame_mut().chunks_exact_mut(4))
                {
                    let (r, g, b) = palette_color_to_rgb(*color);
                    rgba[..3].copy_from_slice(&[r, g, b]);
                    rgba[3] = 255;
                }

                if let Err(error) = pixels.render_with(|encoder, render_target, _context| {
                    let margin = (12.0 * window.scale_factor()) as f32;
                    self.cga_renderer.as_ref().unwrap().render(
                        encoder,
                        render_target,
                        size.width,
                        size.height,
                        margin,
                    )
                }) {
                    self.fail(event_loop, error.into());
                }
            }
            WindowEvent::KeyboardInput {
                device_id: _,
                event,
                is_synthetic,
            } => {
                if self.machine.keyboard_input_mode() == KeyboardInputMode::PcHardware {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if let Some(scan_code) = winit_to_pc_scan_code(code) {
                            // Directly send the raw key to the machine
                            self.machine.enqueue_pc_keyboard_event(
                                scan_code,
                                event.state == ElementState::Pressed,
                            );
                        }
                    }
                    return;
                }

                if is_synthetic || event.state != ElementState::Pressed {
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    let key = winit_to_bios_key_stroke(code, event.text, &self.modifier_state);
                    if let Some(key) = key {
                        self.machine.enqueue_bios_keystroke(key);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifier_state.shift = state.shift_key();
                self.modifier_state.ctrl = state.control_key();
                self.modifier_state.alt = state.alt_key();
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if Instant::now() >= self.next_frame {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            self.next_frame = Instant::now() + FRAME_INTERVAL;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

pub(crate) fn run(machine: &mut Machine) -> anyhow::Result<u8> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        machine,
        pixels: None,
        window: None,
        error: None,
        next_frame: Instant::now(),
        exit_code: None,
        modifier_state: ModifierState {
            shift: false,
            ctrl: false,
            alt: false,
        },
        cga_renderer: None,
        indexed_frame: vec![0; 640 * 200].into_boxed_slice(),
        pacing_start: Instant::now(),
        starting_cycles: 0,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(app.exit_code.unwrap_or(0)),
    }
}
