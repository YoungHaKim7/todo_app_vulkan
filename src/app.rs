//! Application state and winit event handling.

use std::{path::PathBuf, time::Instant};

use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::WindowId,
};

use crate::{
    renderer::{GpuContext, RenderContext},
    todos::{Todos, sanitize},
};

const SAVE_FILE: &str = "todos.txt";

pub(crate) struct App {
    pub(crate) gpu: GpuContext,
    pub(crate) todos: Todos,
    pub(crate) save_path: PathBuf,
    pub(crate) mouse: [f32; 2],
    pub(crate) pending_clicks: Vec<[f32; 2]>,
    pub(crate) cursor_is_pointer: bool,
    pub(crate) dump_done: bool,
    pub(crate) rcx: Option<RenderContext>,
}

impl App {
    pub(crate) fn new(event_loop: &EventLoop<()>) -> Self {
        println!("Vulkan ToDo");
        println!(
            "Controls: type + Enter = add task · click checkbox = toggle · X = delete · scroll = move list · Esc = quit"
        );

        let gpu = GpuContext::new(event_loop);

        let save_path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(SAVE_FILE);

        let todos = Todos::load(&save_path);
        println!(
            "{} task(s) loaded from {}",
            todos.items.len(),
            save_path.display()
        );

        Self {
            gpu,
            todos,
            save_path,
            mouse: [-1000.0; 2],
            pending_clicks: Vec::new(),
            cursor_is_pointer: false,
            dump_done: false,
            rcx: None,
        }
    }

    fn handle_keyboard(&mut self, event: KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if !self.todos.focused {
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Enter) => self.todos.add_task(&self.save_path),
            Key::Named(NamedKey::Backspace) => {
                if self.todos.input.pop().is_some() {
                    self.todos.caret_since = Instant::now();
                }
            }
            Key::Named(NamedKey::Space) => self.type_char(' '),
            Key::Character(text) => {
                for c in text.as_str().chars().filter_map(sanitize) {
                    self.type_char(c);
                }
            }
            _ => {}
        }
    }

    fn type_char(&mut self, c: char) {
        if self.todos.input.chars().count() < 80 {
            self.todos.input.push(c);
            self.todos.caret_since = Instant::now();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.rcx = Some(RenderContext::new(&self.gpu, event_loop));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(_) => {
                if let Some(rcx) = self.rcx.as_mut() {
                    rcx.recreate_swapchain = true;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = [position.x as f32, position.y as f32];
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left && state == ElementState::Released {
                    self.pending_clicks.push(self.mouse);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.todos.scroll =
                    (self.todos.scroll - lines * 40.0).clamp(0.0, self.todos.max_scroll);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Key::Named(NamedKey::Escape) = event.logical_key {
                    if event.state == ElementState::Pressed {
                        event_loop.exit();
                    }
                } else {
                    self.handle_keyboard(event);
                }
            }
            WindowEvent::Focused(false) => {
                self.todos.focused = false;
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.dump_done
            && let Some(path) = std::env::var_os("TODO_DUMP_FRAME")
        {
            self.dump_done = true;
            self.dump_frame(&path.to_string_lossy());
            event_loop.exit();
            return;
        }
        if let Some(rcx) = self.rcx.as_ref() {
            rcx.window.request_redraw();
        }
    }
}
