//! Vulkan ToDo app: an immediate-mode GUI rendered with Vulkano + winit.
//!
//! Module map:
//! - [`app`]      — application state and event handling
//! - [`renderer`] — Vulkan setup and frame rendering
//! - [`ui`]       — immediate-mode GUI core, widgets, and the ToDo screen
//! - [`font`]     — embedded TTF font rasterization (Hack Nerd Font)
//! - [`atlas`]    — glyph atlas packing and metrics
//! - [`input`]    — text input field: caret, selection, and editing operations
//! - [`todos`]    — ToDo model and persistence
//! - [`settings`] — app settings (font size) and persistence
//! - [`shaders`]  — SPIR-V shader modules

mod app;
mod atlas;
mod font;
mod input;
mod renderer;
mod settings;
mod shaders;
mod todos;
mod ui;

use std::error::Error;

use winit::event_loop::EventLoop;

use crate::app::App;

fn main() -> Result<(), impl Error> {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new(&event_loop);

    event_loop.run_app(&mut app)
}
