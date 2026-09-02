//! ToDo item model and persistence to `todos.txt` (one
//! `flag<TAB>priority<TAB>text` line per item).

use std::{fs, path::Path, time::Instant};

use crate::{input::TextField, ui::Rect};

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

/// A task's priority, shown as a colored stripe left of the row's checkbox: red
/// for emergencies, yellow for what is next up, gray for everything else. The
/// discriminant is the save-file digit (0 = gray, 1 = yellow, 2 = red).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Priority {
    /// General: gray stripe, listed last; the default when nothing is entered.
    #[default]
    Low,
    /// Next up: yellow stripe, listed after the emergencies.
    Mid,
    /// Emergency: red stripe, listed on top.
    High,
}

impl Priority {
    /// Sort key: emergencies first, general last (smaller ranks higher).
    fn rank(self) -> u8 {
        match self {
            Priority::High => 0,
            Priority::Mid => 1,
            Priority::Low => 2,
        }
    }

    /// The priority a click on the stripe selects: gray → yellow → red → gray.
    fn next(self) -> Self {
        match self {
            Priority::Low => Priority::Mid,
            Priority::Mid => Priority::High,
            Priority::High => Priority::Low,
        }
    }
}

pub(crate) struct Todo {
    pub(crate) text: String,
    pub(crate) done: bool,
    pub(crate) priority: Priority,
}

fn parse_save_line(line: &str) -> Option<Todo> {
    let mut fields = line.splitn(3, '\t');
    let flag = fields.next()?;
    let mid = fields.next()?;
    // Newer saves are `done<TAB>priority<TAB>text`; older ones `done<TAB>text`
    // with no priority column, which reads back as gray.
    let (priority, text) = match fields.next() {
        Some(text) => (parse_priority(mid), text),
        None => (Priority::Low, mid),
    };
    let text: String = text.chars().filter_map(sanitize).collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(Todo {
        text,
        done: flag.trim() == "1",
        priority,
    })
}

fn parse_priority(s: &str) -> Priority {
    match s.trim() {
        "2" => Priority::High,
        "1" => Priority::Mid,
        _ => Priority::Low,
    }
}

fn encode_save_line(todo: &Todo) -> String {
    format!(
        "{}\t{}\t{}",
        u8::from(todo.done),
        todo.priority as u8,
        todo.text
    )
}

/// Orders items red first, then yellow, then gray. Stable, so tasks keep their
/// relative order within one priority.
fn sort_by_priority(items: &mut [Todo]) {
    items.sort_by_key(|t| t.priority.rank());
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
    /// The task lifted out of the list into the input field for editing, if any. It
    /// returns — done flag and priority intact — when the input is committed (Enter
    /// or the Add/Save button) or the edit is cancelled (Esc).
    pub(crate) editing: Option<Todo>,
}

impl Todos {
    pub(crate) fn load(path: &Path) -> Self {
        let mut items: Vec<Todo> = fs::read_to_string(path)
            .map(|data| data.lines().filter_map(parse_save_line).collect())
            .unwrap_or_default();
        // Saves are written already sorted, but older or hand-edited files may not
        // be; sorting here keeps red on top from the first frame.
        sort_by_priority(&mut items);
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
            editing: None,
        }
    }

    pub(crate) fn save(&self, path: &Path) {
        // A task lifted into the input field for editing stays in the file with its
        // pre-edit text, so a save for an unrelated change cannot drop it.
        let body: String = self
            .items
            .iter()
            .chain(self.editing.iter())
            .map(|t| encode_save_line(t) + "\n")
            .collect();
        let _ = fs::write(path, body);
    }

    pub(crate) fn add_task(&mut self, path: &Path) {
        let text = self.input.text.trim().to_string();
        if self.editing.is_none() && text.is_empty() {
            return;
        }
        let todo = match self.editing.take() {
            // Committing an edit writes the field's text back into the lifted-out
            // task, whose done flag and priority ride along untouched; an empty
            // field hands it back unchanged (a cancel).
            Some(mut edited) => {
                if !text.is_empty() {
                    edited.text = text;
                }
                edited
            }
            // New tasks enter as gray; the sort below is a no-op placement-wise (gray
            // sorts last) but keeps the ordering invariant explicit.
            None => Todo {
                text,
                done: false,
                priority: Priority::Low,
            },
        };
        self.items.push(todo);
        sort_by_priority(&mut self.items);
        self.input.clear();
        self.caret_since = Instant::now();
        self.preedit = None;
        self.save(path);
    }

    /// Lifts a task out of the list into the input field for editing: the field takes
    /// its text with the caret at the end and grabs focus. A task already being edited
    /// returns unchanged — pushed back after the removal, so `i` keeps pointing at the
    /// row this click hit.
    pub(crate) fn begin_edit(&mut self, i: usize) {
        if i >= self.items.len() {
            return;
        }
        let todo = self.items.remove(i);
        if let Some(prev) = self.editing.take() {
            self.items.push(prev);
            sort_by_priority(&mut self.items);
        }
        self.input.clear();
        self.input.insert_str(&todo.text);
        self.editing = Some(todo);
        self.focused = true;
        self.caret_since = Instant::now();
        self.preedit = None;
    }

    /// Returns the task being edited to the list unchanged and empties the field: the
    /// undo for a pencil click. No save is needed — the file still lists the task with
    /// its pre-edit text throughout the edit.
    pub(crate) fn cancel_edit(&mut self) {
        if let Some(todo) = self.editing.take() {
            self.items.push(todo);
            sort_by_priority(&mut self.items);
            self.input.clear();
            self.caret_since = Instant::now();
            self.preedit = None;
        }
    }

    /// Advances the stripe-clicked task's priority (gray → yellow → red → gray)
    /// and re-sorts, so red items jump to the top the moment they are marked.
    pub(crate) fn cycle_priority(&mut self, i: usize, path: &Path) {
        if let Some(todo) = self.items.get_mut(i) {
            todo.priority = todo.priority.next();
        }
        sort_by_priority(&mut self.items);
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
            editing: None,
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
        let todos = with_items(vec![
            todo("buy milk", false, Priority::Low),
            todo("ship release 1.0!", true, Priority::High),
        ]);
        todos.save(&path);
        let loaded = Todos::load(&path);
        assert_eq!(loaded.items.len(), 2);
        // Loading sorts red on top, so the urgent task comes first.
        assert_eq!(loaded.items[0].text, "ship release 1.0!");
        assert!(loaded.items[0].done);
        assert_eq!(loaded.items[0].priority, Priority::High);
        assert_eq!(loaded.items[1].text, "buy milk");
        assert!(!loaded.items[1].done);
        assert_eq!(loaded.items[1].priority, Priority::Low);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_accepts_old_and_new_save_lines() {
        // Older files have no priority column; they read back as gray.
        let old = parse_save_line("1\tlegacy task").unwrap();
        assert_eq!(old.text, "legacy task");
        assert!(old.done);
        assert_eq!(old.priority, Priority::Low);

        let high = parse_save_line("0\t2\turgent").unwrap();
        assert_eq!(high.text, "urgent");
        assert!(!high.done);
        assert_eq!(high.priority, Priority::High);

        let mid = parse_save_line("1\t1\tsoon").unwrap();
        assert!(mid.done);
        assert_eq!(mid.priority, Priority::Mid);

        assert_eq!(
            encode_save_line(&todo("urgent", false, Priority::High)),
            "0\t2\turgent"
        );
    }

    #[test]
    fn sort_keeps_red_on_top_yellow_next_gray_last() {
        let mut items = vec![
            todo("gray a", false, Priority::Low),
            todo("red", false, Priority::High),
            todo("yellow", false, Priority::Mid),
            todo("gray b", false, Priority::Low),
        ];
        sort_by_priority(&mut items);
        let texts: Vec<&str> = items.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["red", "yellow", "gray a", "gray b"]);
    }

    #[test]
    fn cycling_priority_resorts_and_saves() {
        let path =
            std::env::temp_dir().join(format!("vulkan_todo_prio_{}.txt", std::process::id()));
        let mut todos = with_items(vec![
            todo("gray a", false, Priority::Low),
            todo("gray b", false, Priority::Low),
            todo("red!", false, Priority::High),
        ]);
        // gray b → yellow: it moves above both grays but below red.
        todos.cycle_priority(1, &path);
        assert_eq!(todos.items[0].text, "red!");
        assert_eq!(todos.items[1].text, "gray b");
        assert_eq!(todos.items[1].priority, Priority::Mid);
        assert_eq!(todos.items[2].text, "gray a");
        // And again → red: it joins the emergencies on top.
        todos.cycle_priority(1, &path);
        assert_eq!(todos.items[0].text, "red!");
        assert_eq!(todos.items[1].text, "gray b");
        assert_eq!(todos.items[1].priority, Priority::High);
        // The sorted order survived the save/load roundtrip.
        let loaded = Todos::load(&path);
        let texts: Vec<&str> = loaded.items.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["red!", "gray b", "gray a"]);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn edit_round_trips_through_the_input_field() {
        let path =
            std::env::temp_dir().join(format!("vulkan_todo_edit_{}.txt", std::process::id()));
        let mut todos = with_items(vec![
            todo("red!", false, Priority::High),
            todo("buy milk", false, Priority::Low),
        ]);
        todos.begin_edit(1);
        assert_eq!(
            todos.items.len(),
            1,
            "the edited task lifts out of the list"
        );
        assert_eq!(todos.items[0].text, "red!");
        assert_eq!(todos.input.text, "buy milk");
        assert_eq!(
            todos.input.caret,
            "buy milk".len(),
            "the caret starts at the end"
        );
        assert!(todos.focused, "editing grabs focus for the field");
        assert_eq!(
            todos.editing.as_ref().map(|t| t.priority),
            Some(Priority::Low)
        );
        // A save while the edit is pending still keeps the task on disk.
        todos.save(&path);
        assert_eq!(Todos::load(&path).items.len(), 2);

        todos.input.select_all();
        todos.input.insert_str("buy oat milk");
        todos.add_task(&path);
        assert!(todos.editing.is_none());
        assert!(todos.input.text.is_empty(), "committing clears the field");
        let texts: Vec<&str> = todos.items.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            texts,
            ["red!", "buy oat milk"],
            "the task returns in its place"
        );
        // The edited text survived the save/load roundtrip.
        assert_eq!(Todos::load(&path).items[1].text, "buy oat milk");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn cancel_and_empty_commit_restore_the_task() {
        let mut todos = with_items(vec![
            todo("done thing", true, Priority::Low),
            todo("red!", false, Priority::High),
        ]);
        todos.begin_edit(0);
        todos.cancel_edit();
        assert_eq!(todos.items.len(), 2);
        assert!(todos.items[1].done, "cancel keeps the done flag");
        assert!(todos.items[1].text == "done thing");
        assert!(todos.input.text.is_empty());
        assert!(todos.editing.is_none());

        // Committing with an empty field cancels the edit the same way.
        todos.begin_edit(0);
        todos.input.clear();
        todos.add_task(Path::new("/dev/null"));
        assert_eq!(todos.items.len(), 2);
        assert_eq!(todos.items[1].text, "done thing");
        assert!(
            todos.items[1].done,
            "an empty commit keeps the done flag too"
        );
    }

    #[test]
    fn commit_keeps_done_flag_and_priority() {
        let mut todos = with_items(vec![
            todo("ship release 1.0!", true, Priority::High),
            todo("buy milk", false, Priority::Low),
        ]);
        todos.begin_edit(0);
        todos.input.select_all();
        todos.input.insert_str("ship release 1.1!");
        todos.add_task(Path::new("/dev/null"));
        assert_eq!(todos.items[0].text, "ship release 1.1!");
        assert!(todos.items[0].done, "the done flag rides along");
        assert_eq!(
            todos.items[0].priority,
            Priority::High,
            "so does the priority"
        );
    }

    #[test]
    fn editing_another_task_returns_the_first_unchanged() {
        let mut todos = with_items(vec![
            todo("gray a", false, Priority::Low),
            todo("gray b", false, Priority::Low),
        ]);
        todos.begin_edit(0); // lifts "gray a"
        todos.begin_edit(0); // lifts "gray b"; "gray a" goes back unchanged
        assert_eq!(todos.items.len(), 1);
        assert_eq!(todos.items[0].text, "gray a");
        assert_eq!(todos.editing.as_ref().unwrap().text, "gray b");
    }

    fn todo(text: &str, done: bool, priority: Priority) -> Todo {
        Todo {
            text: text.into(),
            done,
            priority,
        }
    }

    fn with_items(items: Vec<Todo>) -> Todos {
        Todos {
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
            editing: None,
        }
    }
}
