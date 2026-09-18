use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use pixels::{Pixels, SurfaceTexture};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use crate::machine::{DiskType, LoadMode, Machine, MachineStepResult};

pub struct App {
    machine: Machine,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    error: Option<anyhow::Error>,
    next_frame: Instant,
    exit_code: Option<u8>,
}

const WIDTH: u32 = 640;
const HEIGHT: u32 = 200;
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
                        .with_inner_size(LogicalSize::new(
                            f64::from(WIDTH) * 2.0,
                            f64::from(HEIGHT) * 2.0,
                        ))
                        .with_min_inner_size(LogicalSize::new(WIDTH as f64, HEIGHT as f64)),
                )?,
            );
            let size = window.inner_size();
            let surface = SurfaceTexture::new(size.width, size.height, window.clone());
            let pixels = Pixels::new(WIDTH, HEIGHT, surface)?;
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
                if let Err(error) = pixels.render() {
                    self.fail(event_loop, error.into());
                }
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

pub(crate) fn run(
    file_path: &str,
    load_mode: LoadMode,
    image_type: Option<DiskType>,
    verbosity: u8,
) -> anyhow::Result<u8> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        machine: Machine::new(file_path, load_mode, image_type, verbosity)?,
        pixels: None,
        window: None,
        error: None,
        next_frame: Instant::now(),
        exit_code: None,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(app.exit_code.unwrap_or(0)),
    }
}
