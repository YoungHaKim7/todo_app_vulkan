//! ToDo item model and persistence to `todos.txt` (one `flag<TAB>text` line per item).

use std::{fs, path::Path, time::Instant};

pub(crate) fn sanitize(c: char) -> Option<char> {
    if c == '\t' {
        Some(' ')
    } else if c.is_ascii_graphic() || c == ' ' || c == '·' || c == '−' {
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
    pub(crate) input: String,
    pub(crate) focused: bool,
    pub(crate) caret_since: Instant,
    pub(crate) scroll: f32,
    pub(crate) max_scroll: f32,
}

impl Todos {
    pub(crate) fn load(path: &Path) -> Self {
        let items = fs::read_to_string(path)
            .map(|data| data.lines().filter_map(parse_save_line).collect())
            .unwrap_or_default();
        Self {
            items,
            input: String::new(),
            focused: false,
            caret_since: Instant::now(),
            scroll: 0.0,
            max_scroll: 0.0,
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
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.items.push(Todo { text, done: false });
        self.input.clear();
        self.caret_since = Instant::now();
        self.save(path);
    }

    pub(crate) fn open_count(&self) -> usize {
        self.items.iter().filter(|t| !t.done).count()
    }

    pub(crate) fn done_count(&self) -> usize {
        self.items.iter().filter(|t| t.done).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_only_renderable_ascii() {
        assert_eq!(sanitize('a'), Some('a'));
        assert_eq!(sanitize(' '), Some(' '));
        assert_eq!(sanitize('\t'), Some(' '));
        assert_eq!(sanitize('\n'), None);
        assert_eq!(sanitize('·'), Some('·'));
        assert_eq!(sanitize('−'), Some('−'));
        assert_eq!(sanitize('\u{e9}'), None);
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
            input: String::new(),
            focused: false,
            caret_since: Instant::now(),
            scroll: 0.0,
            max_scroll: 0.0,
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
