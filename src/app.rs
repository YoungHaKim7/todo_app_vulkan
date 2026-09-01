//! Application state and winit event handling.

use std::{path::PathBuf, time::Instant};

use copypasta::{ClipboardContext, ClipboardProvider, nop_clipboard::NopClipboardContext};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

use crate::{
    font,
    input::TextField,
    renderer::{GpuContext, RenderContext},
    settings::Settings,
    todos::{Todos, sanitize},
};

const SAVE_FILE: &str = "todos.txt";
const SETTINGS_FILE: &str = "settings.txt";

pub(crate) struct App {
    pub(crate) gpu: GpuContext,
    pub(crate) todos: Todos,
    pub(crate) save_path: PathBuf,
    pub(crate) settings: Settings,
    pub(crate) settings_path: PathBuf,
    pub(crate) mouse: [f32; 2],
    pub(crate) pending_clicks: Vec<[f32; 2]>,
    /// Whether the left button is held, and where that press started; the UI uses
    /// this to drive drag selection in the input field.
    pub(crate) mouse_down: bool,
    pub(crate) press: [f32; 2],
    pub(crate) mods: ModifiersState,
    pub(crate) clipboard: Box<dyn ClipboardProvider>,
    pub(crate) cursor_is_pointer: bool,
    pub(crate) dump_done: bool,
    pub(crate) rcx: Option<RenderContext>,
    /// Whether the input field was focused when the window lost focus, so it regains
    /// focus (and stays editable) when the window is focused again.
    pub(crate) field_focus_on_blur: bool,
    /// Whether the platform IME is currently attached to the window; enabled only
    /// while the input field is focused, so Hangul composition reaches the field.
    pub(crate) ime_on: bool,
    /// Caret rect last handed to the IME, to skip redundant position updates.
    pub(crate) ime_area: Option<[f32; 4]>,
}

impl App {
    pub(crate) fn new(event_loop: &EventLoop<()>) -> Self {
        println!("Vulkan ToDo");
        println!(
            "Controls: type + Enter = add task · click/drag in the input = caret/selection · Ctrl+A/C/X/V · Ctrl+Backspace = delete word · click checkbox = toggle · X = delete · scroll = move list · settings: gear (top left) · Esc: close window / quit"
        );

        let gpu = GpuContext::new(event_loop);

        let save_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let save_path = save_dir.join(SAVE_FILE);
        let settings_path = save_dir.join(SETTINGS_FILE);

        let todos = Todos::load(&save_path);
        println!(
            "{} task(s) loaded from {}",
            todos.items.len(),
            save_path.display()
        );
        let settings = Settings::load(&settings_path);

        Self {
            gpu,
            todos,
            save_path,
            settings,
            settings_path,
            mouse: [-1000.0; 2],
            pending_clicks: Vec::new(),
            mouse_down: false,
            press: [-1000.0; 2],
            mods: ModifiersState::empty(),
            // Replaced with the real backend once a window exists (see `resumed`).
            clipboard: Box::new(NopClipboardContext::new().unwrap()),
            cursor_is_pointer: false,
            dump_done: false,
            rcx: None,
            field_focus_on_blur: false,
            ime_on: false,
            ime_area: None,
        }
    }

    fn handle_keyboard(&mut self, event: KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if self.settings.open || !self.todos.focused {
            return;
        }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        match event.logical_key {
            Key::Named(NamedKey::Enter) => self.todos.add_task(&self.save_path),
            Key::Named(NamedKey::Backspace) => {
                self.edit(|f| if ctrl { f.backspace_word() } else { f.backspace() });
            }
            Key::Named(NamedKey::Delete) => {
                self.edit(|f| if ctrl { f.delete_word() } else { f.delete() });
            }
            Key::Named(NamedKey::ArrowLeft) => self.edit(|f| f.move_left(ctrl, shift)),
            Key::Named(NamedKey::ArrowRight) => self.edit(|f| f.move_right(ctrl, shift)),
            Key::Named(NamedKey::Home) => self.edit(|f| f.move_to_start(shift)),
            Key::Named(NamedKey::End) => self.edit(|f| f.move_to_end(shift)),
            Key::Named(NamedKey::Space) => self.type_str(" "),
            // Ctrl chords act on the selection or clipboard; anything else with Ctrl
            // held types nothing rather than inserting a control character.
            Key::Character(text) if ctrl => match text.to_lowercase().as_str() {
                "a" => self.edit(TextField::select_all),
                "c" => self.copy_selection(),
                "x" => self.cut_selection(),
                "v" => self.paste_clipboard(),
                _ => {}
            },
            Key::Character(text) if !self.mods.alt_key() => self.type_str(&text),
            _ => {}
        }
    }

    /// Applies an editing operation to the input field and restarts the caret blink.
    fn edit(&mut self, op: impl FnOnce(&mut TextField)) {
        op(&mut self.todos.input);
        self.todos.caret_since = Instant::now();
    }

    /// Types literal text into the field at the caret, replacing any selection.
    fn type_str(&mut self, s: &str) {
        let typed: String = s.chars().filter_map(sanitize).collect();
        if !typed.is_empty() {
            self.edit(|f| f.insert_str(&typed));
        }
    }

    /// IME events: Hangul (and other script) composition via fcitx5/ibus. `Preedit`
    /// shows the text being composed at the caret; `Commit` types the finished text.
    fn handle_ime(&mut self, ime: Ime) {
        if self.settings.open || !self.todos.focused {
            self.todos.preedit = None;
            return;
        }
        match ime {
            Ime::Enabled | Ime::Disabled => self.todos.preedit = None,
            Ime::Preedit(text, _) => self.todos.preedit = (!text.is_empty()).then_some(text),
            Ime::Commit(text) => {
                self.todos.preedit = None;
                self.type_str(&text);
            }
        }
    }

    /// Attaches or detaches the platform IME as the input field gains or loses focus,
    /// and keeps the composition popup anchored at the caret.
    fn sync_ime(&mut self, window: &Window) {
        let wanted = self.todos.focused && !self.settings.open;
        if wanted != self.ime_on {
            window.set_ime_allowed(wanted);
            self.ime_on = wanted;
        }
        if wanted {
            let a = self.todos.caret_area;
            let area = [a.x, a.y, a.w, a.h];
            if self.ime_area != Some(area) {
                window.set_ime_cursor_area(
                    PhysicalPosition::new(a.x as f64, a.y as f64),
                    PhysicalSize::new(a.w as f64, a.h.max(1.0) as f64),
                );
                self.ime_area = Some(area);
            }
        }
    }

    fn copy_selection(&mut self) {
        if let Some(text) = self.todos.input.selected_text() {
            let text = text.to_string();
            self.clipboard.set_contents(text).ok();
        }
    }

    fn cut_selection(&mut self) {
        if let Some(text) = self.todos.input.selected_text() {
            let text = text.to_string();
            if self.clipboard.set_contents(text).is_ok() {
                self.edit(|f| f.backspace());
            }
        }
    }

    fn paste_clipboard(&mut self) {
        let Ok(text) = self.clipboard.get_contents() else {
            return;
        };
        // Line breaks and tabs become spaces so pasted text stays one line;
        // everything else goes through the same filter as typing.
        let pasted: String = text
            .chars()
            .filter_map(|c| match c {
                '\n' | '\r' | '\t' => Some(' '),
                _ => sanitize(c),
            })
            .collect();
        if !pasted.is_empty() {
            self.edit(|f| f.insert_str(&pasted));
        }
    }
}

/// Builds the clipboard for the session at hand: Wayland when the window runs on a
/// Wayland display, X11 otherwise, and a no-op stub if neither connects.
fn make_clipboard(window: &Window) -> Box<dyn ClipboardProvider> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    if let Ok(handle) = window.display_handle()
        && let RawDisplayHandle::Wayland(display) = handle.as_raw()
    {
        // SAFETY: the pointer is the live Wayland display backing `window`, which
        // outlives the clipboard built from it.
        let clipboard = unsafe {
            copypasta::wayland_clipboard::create_clipboards_from_external(display.display.as_ptr())
        }
        .1;
        return Box::new(clipboard);
    }
    ClipboardContext::new()
        .map(|ctx| Box::new(ctx) as Box<dyn ClipboardProvider>)
        .unwrap_or_else(|_| Box::new(NopClipboardContext::new().unwrap()))
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let rcx = RenderContext::new(&self.gpu, event_loop);
        self.clipboard = make_clipboard(&rcx.window);
        self.rcx = Some(rcx);
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
            WindowEvent::MouseInput { state, button, .. } if button == MouseButton::Left => {
                match state {
                    ElementState::Pressed => {
                        self.mouse_down = true;
                        self.press = self.mouse;
                        // Focusing on press (not just release) means the caret and
                        // drag selection are visible while the button is held.
                        if !self.settings.open && self.todos.field_rect.contains(self.mouse) {
                            self.todos.focused = true;
                        }
                    }
                    ElementState::Released => {
                        self.mouse_down = false;
                        self.pending_clicks.push(self.mouse);
                    }
                }
            }
            WindowEvent::ModifiersChanged(mods) => self.mods = mods.state(),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if !self.settings.open {
                    self.todos.scroll =
                        (self.todos.scroll - lines * 40.0).clamp(0.0, self.todos.max_scroll);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Key::Named(NamedKey::Escape) = event.logical_key {
                    if event.state == ElementState::Pressed {
                        if self.settings.open {
                            self.settings.open = false;
                        } else {
                            event_loop.exit();
                        }
                    }
                } else {
                    self.handle_keyboard(event);
                }
            }
            WindowEvent::Ime(ime) => self.handle_ime(ime),
            WindowEvent::Focused(focused) => {
                if focused {
                    // The field keeps its selection across alt-tab; give focus back
                    // so the selection stays erasable and typing continues.
                    if self.field_focus_on_blur && !self.settings.open {
                        self.todos.focused = true;
                    }
                    self.field_focus_on_blur = false;
                } else {
                    self.field_focus_on_blur = self.todos.focused;
                    self.todos.focused = false;
                    // The IME resets composition when its window loses focus.
                    self.todos.preedit = None;
                }
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
            // Debug overrides for the frame dump: force a font level and/or the settings
            // window open so both states can be rendered headlessly.
            if let Some(level) = std::env::var("TODO_FONT_LEVEL")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
            {
                self.settings.font_level = level.min(font::LEVELS - 1);
            }
            if std::env::var_os("TODO_SETTINGS_OPEN").is_some() {
                self.settings.open = true;
            }
            // Seed the input field (focused) so editing visuals can be rendered
            // headlessly; TODO_INPUT_SELECT additionally selects everything, and
            // TODO_PREEDIT shows a composition string at the caret.
            if let Ok(text) = std::env::var("TODO_INPUT") {
                self.todos.input.clear();
                self.todos.input.insert_str(&text);
                self.todos.focused = true;
                if std::env::var_os("TODO_INPUT_SELECT").is_some() {
                    self.todos.input.select_all();
                }
            }
            if let Ok(preedit) = std::env::var("TODO_PREEDIT") {
                self.todos.preedit = (!preedit.is_empty()).then_some(preedit);
            }
            self.dump_frame(&path.to_string_lossy());
            event_loop.exit();
            return;
        }
        // Attach the IME to the caret between frames; `caret_area` is where the last
        // drawn frame left it.
        if let Some(window) = self.rcx.as_ref().map(|rcx| rcx.window.clone()) {
            self.sync_ime(&window);
            window.request_redraw();
        }
    }
}
