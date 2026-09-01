//! Single-line text input state: the text plus caret, selection, and every editing
//! operation on them.
//!
//! Positions are byte offsets into `text` and always land on char boundaries (the
//! text mixes single-byte ASCII with multi-byte characters such as `·`, `−`, and
//! Hangul, so `±1` byte stepping is never safe). Selection is the range between
//! `anchor` and `caret`; they are equal when nothing is selected. Horizontal
//! scrolling of the field's viewport lives here too, so the renderer can keep the
//! caret visible.

use crate::{atlas, font::Size};

/// Cap on the number of characters an input may hold, matching the previous
/// append-only field.
pub(crate) const MAX_CHARS: usize = 80;

pub(crate) struct TextField {
    pub(crate) text: String,
    /// Caret position (byte offset). One end of the selection.
    pub(crate) caret: usize,
    /// Selection anchor (byte offset): where the selection was started. The other end.
    pub(crate) anchor: usize,
    /// How far the text is scrolled left, in pixels; kept by the renderer so the
    /// caret stays inside the field.
    pub(crate) scroll_x: f32,
}

impl TextField {
    pub(crate) fn new() -> Self {
        Self {
            text: String::new(),
            caret: 0,
            anchor: 0,
            scroll_x: 0.0,
        }
    }

    /// Empties the field and resets caret, selection, and scroll.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        self.scroll_x = 0.0;
    }

    /// The selected byte range, front-to-back; `None` when caret and anchor meet.
    pub(crate) fn selection(&self) -> Option<std::ops::Range<usize>> {
        (self.caret != self.anchor).then(|| self.caret.min(self.anchor)..self.caret.max(self.anchor))
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

    pub(crate) fn move_to_start(&mut self, select: bool) {
        self.set_caret(0, select);
    }

    pub(crate) fn move_to_end(&mut self, select: bool) {
        self.set_caret(self.text.len(), select);
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

    /// Maps a pixel x to the caret position at the closest char boundary (clicks land
    /// before a glyph when they hit its left half). `text_x` is where the text starts
    /// (unscrolled).
    pub(crate) fn caret_from_x(&self, x: f32, text_x: f32, size: Size) -> usize {
        let mut best = 0usize;
        let mut best_d = (x - text_x).abs();
        let mut cx = text_x;
        for (i, c) in self.text.char_indices() {
            cx += atlas::advance(size, c);
            let d = (x - cx).abs();
            if d < best_d {
                best_d = d;
                best = i + c.len_utf8();
            }
        }
        best
    }

    /// Pixel x of the char boundary `i`, relative to the unscrolled text origin
    /// `text_x`.
    pub(crate) fn x_from_byte(&self, i: usize, text_x: f32, size: Size) -> f32 {
        let mut cx = text_x;
        for (j, c) in self.text.char_indices() {
            if j >= i {
                break;
            }
            cx += atlas::advance(size, c);
        }
        cx
    }

    /// First char still (partly) visible when the text is scrolled left by
    /// `scroll_x`: its byte offset plus how many pixels of it are cut off.
    pub(crate) fn visible_start(&self, scroll_x: f32, size: Size) -> (usize, f32) {
        let mut cx = 0.0;
        for (i, c) in self.text.char_indices() {
            let adv = atlas::advance(size, c);
            if cx + adv > scroll_x {
                return (i, scroll_x - cx);
            }
            cx += adv;
        }
        (self.text.len(), 0.0)
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
            scroll_x: 0.0,
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

        f.move_to_start(false);
        assert_eq!(f.caret, 0);
        f.move_to_end(true);
        assert_eq!(f.selected_text(), Some("hello"));

        f.select_all();
        assert_eq!(f.selection(), Some(0..5));
    }

    #[test]
    fn word_moves_jump_words_not_spaces() {
        let mut f = field("one two", 7, 7);
        f.move_left(true, false);
        assert_eq!(f.caret, 4, "Ctrl+Left lands before 'two', not before its spaces");
        f.move_left(true, false);
        assert_eq!(f.caret, 0);
        f.move_right(true, false);
        assert_eq!(f.caret, 3, "Ctrl+Right lands after 'one', not after its spaces");
        f.move_right(true, false);
        assert_eq!(f.caret, 7);
    }

    #[test]
    fn clear_resets_everything() {
        let mut f = field("text", 2, 4);
        f.scroll_x = 12.0;
        f.clear();
        assert_eq!(f.text, "");
        assert_eq!((f.caret, f.anchor, f.scroll_x), (0, 0, 0.0));
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
    fn visible_start_finds_the_first_not_fully_scrolled_char() {
        let size = crate::font::Size::text(crate::font::DEFAULT_LEVEL);
        let f = field("hello", 0, 0);
        let adv = f.x_from_byte(1, 0.0, size);
        assert_eq!(f.visible_start(0.0, size), (0, 0.0));
        let (i, lead) = f.visible_start(adv * 2.5, size);
        assert_eq!(i, 2);
        assert!((lead - adv * 0.5).abs() < 1e-4);
        assert_eq!(f.visible_start(1e6, size), (5, 0.0));
    }

    #[test]
    fn caret_x_mapping_roundtrips_through_glyph_centers() {
        let size = crate::font::Size::text(crate::font::DEFAULT_LEVEL);
        let f = field("hello", 5, 5);
        for i in 0..=5 {
            let x = f.x_from_byte(i, 0.0, size);
            assert_eq!(f.caret_from_x(x, 0.0, size), i, "boundary {i}");
        }
        // Clicking far left/right clamps to the ends.
        assert_eq!(f.caret_from_x(-100.0, 0.0, size), 0);
        assert_eq!(f.caret_from_x(1e6, 0.0, size), 5);
    }
}
