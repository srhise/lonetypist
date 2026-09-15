//! A single-line text field for the modal dialogs.

use crate::cp437;

/// Long enough for a path, short enough to stay inside a framed box.
pub const MAX_LEN: usize = 120;

/// What the caller intends to do with the value once it is entered.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    CreateAtLaunch,
    SaveAs,
    Retrieve,
    /// Where to write the print job instead of sending it to a printer.
    PrintToFile,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    purpose: Purpose,
    title: String,
    label: String,
    value: Vec<char>,
    cursor: usize,
}

impl Input {
    pub fn new(purpose: Purpose, title: &str, label: &str) -> Self {
        Self {
            purpose,
            title: title.to_string(),
            label: label.to_string(),
            value: Vec::new(),
            cursor: 0,
        }
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn value(&self) -> String {
        self.value.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Seed the field, leaving the cursor at the end ready to continue.
    pub fn set_value(&mut self, text: &str) {
        self.value = text.chars().collect();
        self.value.truncate(MAX_LEN);
        self.cursor = self.value.len();
    }

    /// Filenames go through the same CP437 filter as the document, so the
    /// field can never hold a character the screen cannot draw.
    pub fn insert(&mut self, text: &str) {
        for ch in text.chars() {
            if self.value.len() >= MAX_LEN {
                break;
            }
            let Some(accepted) = cp437::accept(ch) else {
                continue;
            };
            for c in accepted.chars() {
                if self.value.len() >= MAX_LEN {
                    break;
                }
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.value.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.value.len() {
            self.value.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.value.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.value.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str) -> Input {
        let mut i = Input::new(Purpose::SaveAs, "Save Document", "Filename:");
        i.set_value(text);
        i
    }

    #[test]
    fn a_new_field_is_empty_with_the_cursor_at_the_start() {
        let i = Input::new(Purpose::CreateAtLaunch, "New", "Document:");
        assert_eq!(i.value(), "");
        assert_eq!(i.cursor(), 0);
    }

    #[test]
    fn setting_a_value_puts_the_cursor_at_the_end() {
        let i = field("notes.txt");
        assert_eq!(i.value(), "notes.txt");
        assert_eq!(i.cursor(), 9, "ready to keep typing");
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut i = field("ab");
        i.insert("c");
        assert_eq!(i.value(), "abc");
        assert_eq!(i.cursor(), 3);
    }

    #[test]
    fn typing_in_the_middle_splits() {
        let mut i = field("ac");
        i.left();
        i.insert("b");
        assert_eq!(i.value(), "abc");
        assert_eq!(i.cursor(), 2);
    }

    #[test]
    fn the_field_rejects_what_the_screen_cannot_draw() {
        let mut i = field("");
        i.insert("a\u{3042}b");
        assert_eq!(i.value(), "ab");
    }

    #[test]
    fn the_field_rejects_newlines_and_tabs() {
        let mut i = field("");
        i.insert("a\nb\tc");
        assert_eq!(i.value(), "abc");
    }

    #[test]
    fn backspace_removes_before_the_cursor() {
        let mut i = field("abc");
        i.backspace();
        assert_eq!(i.value(), "ab");
        assert_eq!(i.cursor(), 2);
    }

    #[test]
    fn backspace_at_the_start_does_nothing() {
        let mut i = field("abc");
        i.home();
        i.backspace();
        assert_eq!(i.value(), "abc");
        assert_eq!(i.cursor(), 0);
    }

    #[test]
    fn delete_removes_after_the_cursor() {
        let mut i = field("abc");
        i.home();
        i.delete();
        assert_eq!(i.value(), "bc");
        assert_eq!(i.cursor(), 0);
    }

    #[test]
    fn delete_at_the_end_does_nothing() {
        let mut i = field("ab");
        i.delete();
        assert_eq!(i.value(), "ab");
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut i = field("ab");
        i.right();
        assert_eq!(i.cursor(), 2, "cannot go past the end");
        i.home();
        i.left();
        assert_eq!(i.cursor(), 0, "cannot go before the start");
    }

    #[test]
    fn home_and_end_jump() {
        let mut i = field("hello");
        i.home();
        assert_eq!(i.cursor(), 0);
        i.end();
        assert_eq!(i.cursor(), 5);
    }

    #[test]
    fn the_field_counts_characters_not_bytes() {
        let mut i = field("");
        i.insert("caf\u{00E9}");
        assert_eq!(i.cursor(), 4, "four characters, five bytes");
        i.backspace();
        assert_eq!(i.value(), "caf");
    }

    #[test]
    fn a_long_value_is_capped() {
        let mut i = field("");
        i.insert(&"x".repeat(500));
        assert!(i.value().chars().count() <= MAX_LEN);
    }

    #[test]
    fn it_remembers_what_it_is_for() {
        let i = field("a");
        assert_eq!(i.purpose(), Purpose::SaveAs);
        assert_eq!(i.title(), "Save Document");
        assert_eq!(i.label(), "Filename:");
    }
}
