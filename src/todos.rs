//! ToDo item model and persistence to `todos.txt` (one `flag<TAB>text` line per item).

use std::{fs, path::Path, time::Instant};

use crate::{
    input::TextField,
    ui::Rect,
};

pub(crate) fn sanitize(c: char) -> Option<char> {
    if c == '\t' {
        Some(' ')
    } else if !c.is_control() {
        // Hangul and any other script pass through; the atlas rasterizes what the
        // fonts cover and falls back to a blank advance otherwise.
        Some(c)
    } else {
        None
    }
}

pub(crate) struct Todo {
    pub(crate) text: String,
    pub(crate) done: bool,
}

fn parse_save_line(line: &str) -> Option<Todo> {
    let (flag, text) = line.split_once('\t')?;
    let text: String = text.chars().filter_map(sanitize).collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(Todo {
        text,
        done: flag.trim() == "1",
    })
}

fn encode_save_line(todo: &Todo) -> String {
    format!("{}\t{}", u8::from(todo.done), todo.text)
}

pub(crate) struct Todos {
    pub(crate) items: Vec<Todo>,
    pub(crate) input: TextField,
    pub(crate) focused: bool,
    pub(crate) caret_since: Instant,
    pub(crate) scroll: f32,
    pub(crate) max_scroll: f32,
    /// The input field's rect from the last drawn frame, so raw mouse presses can be
    /// hit-tested against it between frames.
    pub(crate) field_rect: Rect,
    /// A mouse press started in the input field and is still held (drag selection).
    pub(crate) field_drag: bool,
    /// When and where the input field was last clicked, for double/triple-click
    /// word/all selection.
    pub(crate) last_field_click: Option<(Instant, [f32; 2], u32)>,
    /// Text currently being composed by an IME (Hangul jamo → syllable); drawn at the
    /// caret, underlined, and not part of the field until committed.
    pub(crate) preedit: Option<String>,
    /// The caret's rect on screen from the last drawn frame, so the IME popup can be
    /// positioned at it between frames.
    pub(crate) caret_area: Rect,
}

impl Todos {
    pub(crate) fn load(path: &Path) -> Self {
        let items = fs::read_to_string(path)
            .map(|data| data.lines().filter_map(parse_save_line).collect())
            .unwrap_or_default();
        Self {
            items,
            input: TextField::new(),
            focused: false,
            caret_since: Instant::now(),
            scroll: 0.0,
            max_scroll: 0.0,
            field_rect: Rect {
                x: -1.0,
                y: -1.0,
                w: 0.0,
                h: 0.0,
            },
            field_drag: false,
            last_field_click: None,
            preedit: None,
            caret_area: Rect {
                x: -1.0,
                y: -1.0,
                w: 0.0,
                h: 0.0,
            },
        }
    }

    pub(crate) fn save(&self, path: &Path) {
        let body: String = self
            .items
            .iter()
            .map(|t| encode_save_line(t) + "\n")
            .collect();
        let _ = fs::write(path, body);
    }

    pub(crate) fn add_task(&mut self, path: &Path) {
        let text = self.input.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.items.push(Todo { text, done: false });
        self.input.clear();
        self.caret_since = Instant::now();
        self.preedit = None;
        self.save(path);
    }

    pub(crate) fn open_count(&self) -> usize {
        self.items.iter().filter(|t| !t.done).count()
    }

    pub(crate) fn done_count(&self) -> usize {
        self.items.iter().filter(|t| t.done).count()
    }

    /// Blurs the input field, collapsing any selection and dropping any composition:
    /// a highlight lingering in an unfocused field would look erasable, but
    /// Del/Backspace only work while focused.
    pub(crate) fn blur(&mut self) {
        self.input.anchor = self.input.caret;
        self.focused = false;
        self.preedit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_printable_and_drops_control() {
        assert_eq!(sanitize('a'), Some('a'));
        assert_eq!(sanitize(' '), Some(' '));
        assert_eq!(sanitize('\t'), Some(' '));
        assert_eq!(sanitize('\n'), None);
        assert_eq!(sanitize('·'), Some('·'));
        assert_eq!(sanitize('−'), Some('−'));
        assert_eq!(sanitize('가'), Some('가'), "Hangul syllables are typable");
        assert_eq!(sanitize('한'), Some('한'));
        assert_eq!(sanitize('글'), Some('글'));
        assert_eq!(sanitize('\u{7}'), None, "control characters are dropped");
    }

    #[test]
    fn blur_collapses_selection() {
        let mut todos = Todos {
            items: Vec::new(),
            input: TextField::new(),
            focused: true,
            caret_since: Instant::now(),
            scroll: 0.0,
            max_scroll: 0.0,
            field_rect: Rect {
                x: -1.0,
                y: -1.0,
                w: 0.0,
                h: 0.0,
            },
            field_drag: false,
            last_field_click: None,
            preedit: None,
            caret_area: Rect {
                x: -1.0,
                y: -1.0,
                w: 0.0,
                h: 0.0,
            },
        };
        todos.input.insert_str("hello");
        todos.input.select_all();
        assert_eq!(todos.input.selection(), Some(0..5));

        todos.blur();
        assert!(!todos.focused);
        assert!(
            todos.input.selection().is_none(),
            "no highlight may outlive focus"
        );
        assert_eq!(todos.input.text, "hello", "blur keeps the text");
    }

    #[test]
    fn save_file_roundtrip() {
        let path =
            std::env::temp_dir().join(format!("vulkan_todo_test_{}.txt", std::process::id()));
        let todos = Todos {
            items: vec![
                Todo {
                    text: "buy milk".into(),
                    done: false,
                },
                Todo {
                    text: "ship release 1.0!".into(),
                    done: true,
                },
            ],
            input: TextField::new(),
            focused: false,
            caret_since: Instant::now(),
            scroll: 0.0,
            max_scroll: 0.0,
            field_rect: Rect {
                x: -1.0,
                y: -1.0,
                w: 0.0,
                h: 0.0,
            },
            field_drag: false,
            last_field_click: None,
            preedit: None,
            caret_area: Rect {
                x: -1.0,
                y: -1.0,
                w: 0.0,
                h: 0.0,
            },
        };
        todos.save(&path);
        let loaded = Todos::load(&path);
        assert_eq!(loaded.items.len(), 2);
        assert_eq!(loaded.items[0].text, "buy milk");
        assert!(!loaded.items[0].done);
        assert_eq!(loaded.items[1].text, "ship release 1.0!");
        assert!(loaded.items[1].done);
        let _ = fs::remove_file(&path);
    }
}
