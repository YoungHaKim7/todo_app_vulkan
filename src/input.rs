//! Text input state: the text plus caret, selection, and every editing operation
//! on them.
//!
//! Positions are byte offsets into `text` and always land on char boundaries (the
//! text mixes single-byte ASCII with multi-byte characters such as `·`, `−`, and
//! Hangul, so `±1` byte stepping is never safe). Selection is the range between
//! `anchor` and `caret`; they are equal when nothing is selected. [`Wrap`] is the
//! text broken into the lines the field shows it on: the renderer builds one per
//! frame and leaves it on the field, where the arrow keys pick it up for
//! visual-line moves.

use crate::{
    atlas,
    font::{DEFAULT_LEVEL, Size},
};

/// Cap on the number of characters an input may hold, matching the previous
/// append-only field.
pub(crate) const MAX_CHARS: usize = 80;

/// The layout of a string wrapped to a width: the byte offsets where each line
/// starts. Built by the renderer each frame; also kept on [`TextField`] for
/// visual-line navigation.
pub(crate) struct Wrap {
    /// Byte offset of the first char of each line; starts at 0 and ascends.
    starts: Vec<usize>,
    /// Font size the line widths were measured at.
    size: Size,
}

impl Default for Wrap {
    fn default() -> Self {
        Self {
            starts: vec![0],
            size: Size::text(DEFAULT_LEVEL),
        }
    }
}

impl Wrap {
    /// Wraps `s` to lines of at most `max_w` pixels. A line breaks after its last
    /// space when one is pending; a run of non-spaces wider than the line breaks at
    /// whatever char overflows. Spaces at a break hang past the line's right edge
    /// rather than opening the next line with a gap.
    pub(crate) fn new(s: &str, max_w: f32, size: Size) -> Self {
        let mut starts = vec![0usize];
        let mut line_start = 0usize;
        let mut cx = 0.0;
        let mut break_at: Option<usize> = None; // byte just past the last space
        for (i, c) in s.char_indices() {
            let adv = atlas::advance(size, c);
            if c != ' ' {
                while cx + adv > max_w && i > line_start {
                    let start = break_at.filter(|&b| b > line_start).unwrap_or(i);
                    starts.push(start);
                    line_start = start;
                    cx = s[start..i].chars().map(|ch| atlas::advance(size, ch)).sum();
                    break_at = None;
                }
            }
            if c == ' ' {
                break_at = Some(i + 1);
            }
            cx += adv;
        }
        Self { starts, size }
    }

    /// Number of lines (`>= 1`, also for empty text).
    pub(crate) fn lines(&self) -> usize {
        self.starts.len()
    }

    /// The line the char boundary `i` sits on; a boundary right at a break belongs
    /// to the line starting there, so that is where the caret shows.
    pub(crate) fn line_of(&self, i: usize) -> usize {
        self.starts.partition_point(|&start| start <= i) - 1
    }

    /// Byte offset of the first char of line `n`.
    pub(crate) fn line_start(&self, n: usize) -> usize {
        self.starts[n.min(self.starts.len() - 1)]
    }

    /// Byte offset just past the last char of line `n`; on a wrapped line this is
    /// the next line's start, spaces eaten by the wrap included.
    pub(crate) fn line_end(&self, s: &str, n: usize) -> usize {
        self.starts.get(n + 1).copied().unwrap_or(s.len())
    }

    /// Like [`Wrap::line_end`], but backed off past any spaces hanging off a
    /// wrapped line's right edge: where a caret on that line should show.
    fn visible_end(&self, s: &str, n: usize) -> usize {
        let mut end = self.line_end(s, n);
        if n + 1 < self.starts.len() {
            let start = self.line_start(n);
            while end > start && s[..end].ends_with(' ') {
                end -= 1; // a space is one byte, so this stays on a boundary
            }
        }
        end
    }

    /// Pixel x of the char boundary `i` within its line: where it renders (a
    /// boundary at a wrap renders as the next line's column 0).
    pub(crate) fn x_at(&self, s: &str, i: usize) -> f32 {
        self.x_in_line(s, i, self.line_of(i))
    }

    /// Pixel x of the char boundary `i` as a position on line `n`, even when `i`
    /// is the boundary the next line starts at — a run ending there (selection,
    /// preedit underline) spans the whole of line `n`, not nothing.
    pub(crate) fn x_in_line(&self, s: &str, i: usize, n: usize) -> f32 {
        let i = i.min(s.len());
        let start = self.line_start(n);
        let mut cx = 0.0;
        for (j, c) in s[start..].char_indices() {
            if start + j >= i {
                break;
            }
            cx += atlas::advance(self.size, c);
        }
        cx
    }

    /// Caret byte offset closest to pixel `x` (measured from the line's left edge)
    /// on line `n`; clicks land before a glyph when they hit its left half.
    pub(crate) fn caret_at(&self, s: &str, x: f32, n: usize) -> usize {
        let start = self.line_start(n);
        let end = self.line_end(s, n);
        let mut best = start;
        let mut best_d = x.abs();
        let mut cx = 0.0;
        for (j, c) in s[start..end].char_indices() {
            cx += atlas::advance(self.size, c);
            let d = (x - cx).abs();
            if d < best_d {
                best_d = d;
                best = start + j + c.len_utf8();
            }
        }
        if best == end {
            best = self.visible_end(s, n); // clicks past a wrap show at that line's end
        }
        best
    }
}

pub(crate) struct TextField {
    pub(crate) text: String,
    /// Caret position (byte offset). One end of the selection.
    pub(crate) caret: usize,
    /// Selection anchor (byte offset): where the selection was started. The other end.
    pub(crate) anchor: usize,
    /// The text wrapped to the field's width, from the last drawn frame; powers
    /// visual-line navigation (Up/Down, Home/End).
    pub(crate) wrap: Wrap,
}

impl TextField {
    pub(crate) fn new() -> Self {
        Self {
            text: String::new(),
            caret: 0,
            anchor: 0,
            wrap: Wrap::default(),
        }
    }

    /// Empties the field and resets caret and selection.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
    }

    /// The selected byte range, front-to-back; `None` when caret and anchor meet.
    pub(crate) fn selection(&self) -> Option<std::ops::Range<usize>> {
        (self.caret != self.anchor)
            .then(|| self.caret.min(self.anchor)..self.caret.max(self.anchor))
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    /// Byte offset of the char boundary at or before `i`.
    fn floor_boundary(&self, i: usize) -> usize {
        let mut i = i.min(self.text.len());
        while !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// Replaces the selection (if any) with `s`, truncating so the field holds at
    /// most [`MAX_CHARS`] characters; the caret lands after the inserted text.
    pub(crate) fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        let room = MAX_CHARS.saturating_sub(self.text.chars().count());
        let tail: String = s.chars().take(room).collect();
        self.text.insert_str(self.caret, &tail);
        self.caret += tail.len();
        self.anchor = self.caret;
    }

    fn delete_selection(&mut self) {
        if let Some(r) = self.selection() {
            self.text.replace_range(r.clone(), "");
            self.caret = r.start;
            self.anchor = self.caret;
        }
    }

    /// Deletes the selection if present, otherwise the char before the caret.
    pub(crate) fn backspace(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let start = self.prev_boundary(self.caret);
        if start < self.caret {
            self.text.replace_range(start..self.caret, "");
            self.caret = start;
            self.anchor = start;
        }
    }

    /// Deletes the selection if present, otherwise the word (and any spaces before
    /// it) left of the caret.
    pub(crate) fn backspace_word(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let start = self.word_start_before(self.caret);
        if start < self.caret {
            self.text.replace_range(start..self.caret, "");
            self.caret = start;
            self.anchor = start;
        }
    }

    /// Deletes the selection if present, otherwise the char after the caret.
    pub(crate) fn delete(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let end = self.next_boundary(self.caret);
        if end > self.caret {
            self.text.replace_range(self.caret..end, "");
            self.anchor = self.caret;
        }
    }

    /// Deletes the selection if present, otherwise the word (and any spaces after
    /// it) right of the caret.
    pub(crate) fn delete_word(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let end = self.word_end_after(self.caret);
        if end > self.caret {
            self.text.replace_range(self.caret..end, "");
            self.anchor = self.caret;
        }
    }

    /// Moves the caret left by one char (or one word); with `select`, the anchor
    /// stays put so the selection extends instead.
    pub(crate) fn move_left(&mut self, word: bool, select: bool) {
        self.set_caret(
            if word {
                self.word_start_before(self.caret)
            } else {
                self.prev_boundary(self.caret)
            },
            select,
        );
    }

    /// Moves the caret right by one char (or one word); with `select`, the anchor
    /// stays put so the selection extends instead.
    pub(crate) fn move_right(&mut self, word: bool, select: bool) {
        self.set_caret(
            if word {
                self.word_end_after(self.caret)
            } else {
                self.next_boundary(self.caret)
            },
            select,
        );
    }

    /// Moves the caret one wrapped line up, keeping its horizontal position; with
    /// `select`, the anchor stays put so the selection extends instead. Nothing
    /// moves on a single line; on the first of several the caret goes to the start
    /// of the text.
    pub(crate) fn move_up(&mut self, select: bool) {
        if self.wrap.lines() < 2 {
            return;
        }
        let n = self.wrap.line_of(self.caret);
        if n == 0 {
            self.set_caret(0, select);
            return;
        }
        let x = self.wrap.x_at(&self.text, self.caret);
        let target = self.wrap.caret_at(&self.text, x, n - 1);
        self.set_caret(target, select);
    }

    /// Moves the caret one wrapped line down, keeping its horizontal position; with
    /// `select`, the anchor stays put so the selection extends instead. On the last
    /// line the caret goes to the end of the text.
    pub(crate) fn move_down(&mut self, select: bool) {
        if self.wrap.lines() < 2 {
            return;
        }
        let n = self.wrap.line_of(self.caret);
        if n + 1 >= self.wrap.lines() {
            self.set_caret(self.text.len(), select);
            return;
        }
        let x = self.wrap.x_at(&self.text, self.caret);
        let target = self.wrap.caret_at(&self.text, x, n + 1);
        self.set_caret(target, select);
    }

    /// Moves the caret to the start of its wrapped line; with `select`, the anchor
    /// stays put so the selection extends instead.
    pub(crate) fn move_to_line_start(&mut self, select: bool) {
        let start = self.wrap.line_start(self.wrap.line_of(self.caret));
        self.set_caret(start, select);
    }

    /// Moves the caret to the end of its wrapped line, before any spaces hanging
    /// past a wrap; with `select`, the anchor stays put so the selection extends
    /// instead. On the last line this is the end of the text.
    pub(crate) fn move_to_line_end(&mut self, select: bool) {
        let end = self
            .wrap
            .visible_end(&self.text, self.wrap.line_of(self.caret));
        self.set_caret(end, select);
    }

    pub(crate) fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// Selects the word the caret sits in or next to (a run of spaces counts as its
    /// own word); used for double-click. With the caret between two words, the right
    /// one wins, matching what editors pick.
    pub(crate) fn select_word_around(&mut self) {
        let mut start = self.caret;
        if start < self.text.len() && self.char_at(start) != ' ' {
            while start > 0 && self.char_before(start) != ' ' {
                start = self.prev_boundary(start);
            }
        } else {
            while start > 0 && self.char_before(start) == ' ' {
                start = self.prev_boundary(start);
            }
            while start > 0 && self.char_before(start) != ' ' {
                start = self.prev_boundary(start);
            }
        }
        let mut end = start;
        while end < self.text.len() && self.char_at(end) != ' ' {
            end = self.next_boundary(end);
        }
        self.anchor = start;
        self.caret = end;
    }

    /// Byte offset of the char boundary before `i` (or `i` itself at the start).
    fn prev_boundary(&self, i: usize) -> usize {
        let i = self.floor_boundary(i);
        (i.checked_sub(1).map(|j| self.floor_boundary(j))).unwrap_or(0)
    }

    /// Byte offset of the char boundary after `i` (or `i` itself at the end).
    fn next_boundary(&self, i: usize) -> usize {
        let i = self.floor_boundary(i);
        if i >= self.text.len() {
            return i;
        }
        let mut j = i + 1;
        while !self.text.is_char_boundary(j) {
            j += 1;
        }
        j
    }

    /// Start of the word left of `i`: skips a run of spaces, then a run of
    /// non-spaces. Standard Ctrl+Backspace/Ctrl+Left behavior.
    fn word_start_before(&self, i: usize) -> usize {
        let mut i = self.floor_boundary(i);
        while i > 0 && self.char_before(i) == ' ' {
            i = self.prev_boundary(i);
        }
        while i > 0 && self.char_before(i) != ' ' {
            i = self.prev_boundary(i);
        }
        i
    }

    /// End of the word right of `i`: skips a run of spaces, then a run of
    /// non-spaces. Standard Ctrl+Delete/Ctrl+Right behavior.
    fn word_end_after(&self, i: usize) -> usize {
        let mut i = self.floor_boundary(i);
        while i < self.text.len() && self.char_at(i) == ' ' {
            i = self.next_boundary(i);
        }
        while i < self.text.len() && self.char_at(i) != ' ' {
            i = self.next_boundary(i);
        }
        i
    }

    fn char_at(&self, i: usize) -> char {
        self.text[i..].chars().next().unwrap_or('\0')
    }

    fn char_before(&self, i: usize) -> char {
        self.text[..i].chars().next_back().unwrap_or('\0')
    }

    fn set_caret(&mut self, i: usize, select: bool) {
        self.caret = self.floor_boundary(i);
        if !select {
            self.anchor = self.caret;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str, caret: usize, anchor: usize) -> TextField {
        assert!(text.is_char_boundary(caret) && text.is_char_boundary(anchor));
        TextField {
            text: text.to_string(),
            caret,
            anchor,
            wrap: Wrap::default(),
        }
    }

    #[test]
    fn insert_replaces_selection_and_moves_caret() {
        let mut f = field("hello world", 5, 11);
        f.insert_str("there");
        assert_eq!(f.text, "hellothere");
        assert_eq!(f.caret, 10);
        assert_eq!(f.anchor, 10);
        assert!(f.selection().is_none());
    }

    #[test]
    fn insert_is_capped_at_max_chars() {
        let mut f = field(&"x".repeat(78), 78, 78);
        f.insert_str("abcdef");
        assert_eq!(f.text.chars().count(), MAX_CHARS);
        assert_eq!(f.caret, MAX_CHARS);
    }

    #[test]
    fn backspace_deletes_char_then_selection() {
        let mut f = field("hi·o", 4, 4); // caret after the 2-byte '·'
        f.backspace();
        assert_eq!(f.text, "hio");
        assert_eq!(f.caret, 2);
        f.anchor = 0; // select "hi"
        f.backspace();
        assert_eq!(f.text, "o");
        assert_eq!(f.caret, 0);
    }

    #[test]
    fn backspace_word_removes_trailing_spaces_and_word() {
        let mut f = field("one two  ", 9, 9);
        f.backspace_word();
        assert_eq!(f.text, "one ");
        f.backspace_word();
        assert_eq!(f.text, "");
    }

    #[test]
    fn delete_and_delete_word_work_forward() {
        let mut f = field("ab cd", 0, 0);
        f.delete_word();
        assert_eq!(f.text, " cd");
        f.delete(); // drops the leading space
        f.delete(); // drops 'c'
        assert_eq!(f.text, "d");
        // Deleting the selection beats deleting forward.
        f.caret = 0;
        f.anchor = 1;
        f.delete();
        assert_eq!(f.text, "");
    }

    #[test]
    fn word_ops_stop_at_multibyte_boundaries() {
        let mut f = field("a − b", 5, 5); // caret after the 3-byte '−'
        f.backspace_word();
        assert_eq!(f.text, "a  b");
        let mut f = field("a · b", 2, 2);
        f.move_right(false, false);
        assert_eq!(f.caret, 4); // skipped the 2-byte '·' completely
        f.backspace();
        assert_eq!(f.text, "a  b");
    }

    #[test]
    fn moves_collapse_or_extend_selection() {
        let mut f = field("hello", 5, 5);
        f.move_left(false, false);
        assert_eq!((f.caret, f.anchor), (4, 4));
        f.move_left(false, true);
        f.move_left(false, true);
        assert_eq!((f.caret, f.anchor), (2, 4));
        assert_eq!(f.selected_text(), Some("ll"));
        f.move_right(true, false);
        assert_eq!((f.caret, f.anchor), (5, 5));

        f.move_to_line_start(false);
        assert_eq!(f.caret, 0);
        f.move_to_line_end(true);
        assert_eq!(f.selected_text(), Some("hello"));

        f.select_all();
        assert_eq!(f.selection(), Some(0..5));
    }

    #[test]
    fn word_moves_jump_words_not_spaces() {
        let mut f = field("one two", 7, 7);
        f.move_left(true, false);
        assert_eq!(
            f.caret, 4,
            "Ctrl+Left lands before 'two', not before its spaces"
        );
        f.move_left(true, false);
        assert_eq!(f.caret, 0);
        f.move_right(true, false);
        assert_eq!(
            f.caret, 3,
            "Ctrl+Right lands after 'one', not after its spaces"
        );
        f.move_right(true, false);
        assert_eq!(f.caret, 7);
    }

    #[test]
    fn clear_resets_everything() {
        let mut f = field("text", 2, 4);
        f.clear();
        assert_eq!(f.text, "");
        assert_eq!((f.caret, f.anchor), (0, 0));
    }

    #[test]
    fn double_click_word_selection_spans_the_word_at_the_caret() {
        let mut f = field("one two", 5, 5); // inside "two"
        f.select_word_around();
        assert_eq!(f.selected_text(), Some("two"));
        let mut f = field("one two", 4, 4); // at the start of "two"
        f.select_word_around();
        assert_eq!(f.selected_text(), Some("two"));
        let mut f = field("one two", 7, 7); // at the end: the left word wins
        f.select_word_around();
        assert_eq!(f.selected_text(), Some("two"));
        let mut f = field("one  two", 4, 4); // in the gap between the words
        f.select_word_around();
        assert_eq!(f.selected_text(), Some("one"));
    }

    #[test]
    fn wrap_breaks_after_spaces_and_midword() {
        let size = Size::text(DEFAULT_LEVEL);
        let adv = atlas::advance(size, 'M');
        // Four-and-a-half chars per line: the space after four M's cannot fit, so
        // the wrap lands after it and the second line starts at the first M.
        let w = Wrap::new("MMMM MMMM", adv * 4.5, size);
        assert_eq!(w.lines(), 2);
        assert_eq!(w.line_start(1), 5);
        // A run of non-spaces wider than the line breaks at the overflowing char.
        let w = Wrap::new("MMMMMMMM", adv * 4.5, size);
        assert_eq!(w.lines(), 2);
        assert_eq!(w.line_start(1), 4);
        let w = Wrap::new(&"M".repeat(12), adv * 4.5, size);
        assert_eq!(w.lines(), 3);
        // Short and empty texts stay one line.
        assert_eq!(Wrap::new("hi", adv * 4.5, size).lines(), 1);
        assert_eq!(Wrap::new("", adv * 4.5, size).lines(), 1);
    }

    #[test]
    fn wrap_breaks_on_char_boundaries_with_multibyte_chars() {
        let size = Size::text(DEFAULT_LEVEL);
        let dot = atlas::advance(size, '·'); // 2 bytes per char
        let text = "··· ···";
        let w = Wrap::new(text, dot * 3.5, size);
        assert_eq!(w.lines(), 2);
        assert_eq!(&text[w.line_start(0)..w.line_end(text, 0)], "··· ");
        assert_eq!(&text[w.line_start(1)..], "···");
    }

    #[test]
    fn wrap_line_of_gives_breaks_to_the_lower_line() {
        let size = Size::text(DEFAULT_LEVEL);
        let adv = atlas::advance(size, 'M');
        let w = Wrap::new("MMMM MMMM", adv * 4.5, size);
        assert_eq!(w.line_of(0), 0);
        assert_eq!(w.line_of(4), 0);
        assert_eq!(w.line_of(5), 1, "the boundary at the break starts line 1");
        assert_eq!(w.line_of(9), 1, "the end of the text closes line 1");
        // The same boundary measures as column 0 of the lower line (where the
        // caret renders) but as the full width of the upper one (so a selection
        // or underline ending there spans that line).
        assert_eq!(w.x_at("MMMM MMMM", 5), 0.0);
        assert!((w.x_in_line("MMMM MMMM", 5, 0) - adv * 5.0).abs() < 1e-4);
    }

    #[test]
    fn wrap_x_and_caret_at_roundtrip_through_glyph_centers() {
        let size = Size::text(DEFAULT_LEVEL);
        let adv = atlas::advance(size, 'M');
        let text = "MMMMMMMM";
        let w = Wrap::new(text, adv * 4.5, size); // two lines of "MMMM"
        for n in 0..2 {
            // The wrap boundary itself belongs to the lower line, so only the last
            // line roundtrips its end boundary.
            let k_end = if n + 1 == w.lines() { 4 } else { 3 };
            for k in 0..=k_end {
                let i = w.line_start(n) + k;
                let x = w.x_at(text, i);
                assert_eq!(w.caret_at(text, x, n), i, "line {n} boundary {k}");
            }
        }
        // Clicking far left/right of a line clamps to its ends (a wrapped line's
        // right end backs off the space hanging past it).
        assert_eq!(w.caret_at(text, -100.0, 0), 0);
        assert_eq!(w.caret_at(text, 1e6, 0), 4);
        assert_eq!(w.caret_at(text, 1e6, 1), 8);
    }

    #[test]
    fn up_down_home_end_move_by_wrapped_line() {
        let size = Size::text(DEFAULT_LEVEL);
        let adv = atlas::advance(size, 'M');
        let mut f = field(&"M".repeat(10), 10, 10);
        f.wrap = Wrap::new(&f.text, adv * 4.5, size); // "MMMM" / "MMMM" / "MM"
        f.move_up(false);
        assert_eq!(f.caret, 6, "up keeps the pixel column");
        f.move_down(false);
        assert_eq!(f.caret, 10, "down on the last line goes to the text end");
        f.move_up(false);
        f.move_up(false);
        assert_eq!(f.caret, 2);
        f.move_up(false);
        assert_eq!(f.caret, 0, "up on the first line goes to the start");
        f.move_down(false);
        assert_eq!(f.caret, 4, "down from the start keeps the column");
        f.move_to_line_end(false);
        assert_eq!(f.caret, 8, "the middle line ends at the wrap boundary");
        f.move_to_line_start(false);
        assert_eq!(f.caret, 8, "which Home reads as the last line's start");
        f.move_to_line_end(false);
        assert_eq!(f.caret, 10, "end on the last line reaches the text end");

        // Selecting variants extend the selection instead of collapsing it.
        let mut f = field(&"M".repeat(8), 8, 8);
        f.wrap = Wrap::new(&f.text, adv * 4.5, size);
        f.move_up(true);
        assert_eq!(f.selected_text(), Some("MMMM"));

        // A single line has nothing above or below.
        let mut f = field("hello", 2, 2);
        f.move_up(true);
        f.move_down(true);
        assert_eq!((f.caret, f.anchor), (2, 2), "nothing moves on one line");
    }

    #[test]
    fn end_backs_off_spaces_hanging_past_a_wrap() {
        let size = Size::text(DEFAULT_LEVEL);
        let adv = atlas::advance(size, 'M');
        let mut f = field("MMMM MMM", 8, 8);
        f.wrap = Wrap::new(&f.text, adv * 4.5, size); // "MMMM " / "MMM"
        f.move_up(false); // the end of the text sits two M's into line 1
        assert_eq!(f.caret, 3);
        f.move_to_line_end(false);
        assert_eq!(f.caret, 4, "End stops before the wrap's trailing space");
        assert_eq!(f.wrap.line_of(f.caret), 0, "and stays on the first line");
    }
}
