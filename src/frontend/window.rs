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

use crate::frontend::cga_renderer::CgaRenderer;
use crate::frontend::keyboard::{ModifierState, translate_raw_key, translate_winit_key};
use crate::machine::{LoadMode, Machine, MachineStepResult};

pub struct App {
    machine: Machine,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    error: Option<anyhow::Error>,
    next_frame: Instant,
    exit_code: Option<u8>,
    modifier_state: ModifierState,
    cga_renderer: Option<CgaRenderer>,
}

const WIDTH: u32 = 640;
const HEIGHT: u32 = 200;
const CORRECTED_HEIGHT: u32 = 480;
const WINDOW_TITLE: &str = "PC86";
const FRAME_INTERVAL: Duration = Duration::from_micros(16_700); // 60 Hz
const EXECUTION_BUDGET: Duration = Duration::from_millis(2);

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
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
                while self.exit_code.is_none() && Instant::now() < deadline {
                    match self.machine.step() {
                        Ok(MachineStepResult::Continue) => {}
                        Ok(MachineStepResult::WaitingForInput) => break,
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
                self.machine.render_video(pixels.frame_mut());
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
                if self.machine.load_mode() == LoadMode::Rom {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if let Some(scan_code) = translate_raw_key(code) {
                            // Directly send the raw key to the machine
                            self.machine.enqueue_raw_key_event(
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
                    let key = translate_winit_key(code, event.text, &self.modifier_state);
                    if let Some(key) = key {
                        self.machine.enqueue_input_character(key);
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

pub(crate) fn run(machine: Machine) -> anyhow::Result<u8> {
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
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(app.exit_code.unwrap_or(0)),
    }
}
