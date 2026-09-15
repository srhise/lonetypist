//! Modal boxes drawn in the CP437 double-line style.

use crate::input::Input;
use crate::menu::MenuState;
use crate::vga::Screen;

/// CP437 double-line frame pieces.
const TL: u8 = 0xC9;
const TR: u8 = 0xBB;
const BL: u8 = 0xC8;
const BR: u8 = 0xBC;
const H: u8 = 0xCD;
const V: u8 = 0xBA;

/// Why a confirmation is open, so the caller knows what to do with the
/// answer. Quit, New, and Open all ask the same question but follow
/// through differently.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    QuitUnsaved,
    NewUnsaved,
    OpenUnsaved,
    Recover,
}

/// What currently has the keyboard. `None` means the document does.
#[derive(Clone, Debug, PartialEq)]
pub enum Overlay {
    None,
    /// The Alt-= bar, with or without a dropdown pulled down.
    Menu(MenuState),
    /// A modal text field: naming, saving, retrieving.
    Field(Input),
    Message {
        title: String,
        body: String,
        /// Errors get the red box; ordinary information does not.
        danger: bool,
    },
    Confirm {
        prompt: Prompt,
        body: String,
    },
    /// The Shift-F7 print screen. It takes the whole display, as
    /// WordPerfect's did.
    Print {
        /// The default printer, or `None` when the system has none.
        printer: Option<String>,
    },
}

/// Draw a framed, filled box. Coordinates are the outer rectangle.
pub fn draw_box(screen: &mut Screen, x: usize, y: usize, w: usize, h: usize, fg: u8, bg: u8) {
    if w < 2 || h < 2 {
        return;
    }
    for row in 0..h {
        for col in 0..w {
            let glyph = match (row, col) {
                (0, 0) => TL,
                (0, c) if c == w - 1 => TR,
                (r, 0) if r == h - 1 => BL,
                (r, c) if r == h - 1 && c == w - 1 => BR,
                (0, _) => H,
                (r, _) if r == h - 1 => H,
                (_, 0) => V,
                (_, c) if c == w - 1 => V,
                _ => 0x20,
            };
            screen.set(x + col, y + row, glyph, fg, bg);
        }
    }
}

/// Centre a box around `title` and a body that may contain newlines.
pub fn draw_centered(screen: &mut Screen, title: &str, body: &str, fg: u8, bg: u8) {
    let body_lines: Vec<&str> = body.lines().collect();
    let widest = body_lines
        .iter()
        .map(|l| l.chars().count())
        .chain(std::iter::once(title.chars().count()))
        .max()
        .unwrap_or(0);

    let width = (widest + 6).clamp(24, screen.cols());
    let height = (body_lines.len() + 4).min(screen.rows());
    let x = (screen.cols() - width) / 2;
    let y = (screen.rows().saturating_sub(height)) / 2;

    draw_box(screen, x, y, width, height, fg, bg);

    if !title.is_empty() {
        let t = format!(" {title} ");
        let tx = x + (width.saturating_sub(t.chars().count())) / 2;
        screen.put_str(tx, y, &t, fg, bg);
    }
    // The block is centred, but every line starts at the same column:
    // centring each line on its own would scramble aligned columns.
    let block_x = x + (width.saturating_sub(widest)) / 2;
    for (i, line) in body_lines.iter().enumerate() {
        screen.put_str(block_x, y + 2 + i, line, fg, bg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cp437;
    use crate::vga::Mode;

    fn row_text(s: &Screen, row: usize) -> String {
        (0..s.cols())
            .map(|c| cp437::decode(s.cell(c, row).glyph))
            .collect()
    }

    #[test]
    fn a_box_has_all_four_corners() {
        let mut s = Screen::new(Mode::Text80x25);
        s.clear(7, 1);
        draw_box(&mut s, 2, 2, 10, 5, 15, 1);
        assert_eq!(s.cell(2, 2).glyph, TL);
        assert_eq!(s.cell(11, 2).glyph, TR);
        assert_eq!(s.cell(2, 6).glyph, BL);
        assert_eq!(s.cell(11, 6).glyph, BR);
    }

    #[test]
    fn a_box_has_continuous_edges() {
        let mut s = Screen::new(Mode::Text80x25);
        s.clear(7, 1);
        draw_box(&mut s, 0, 0, 6, 4, 15, 1);
        for c in 1..5 {
            assert_eq!(s.cell(c, 0).glyph, H, "top edge at {c}");
            assert_eq!(s.cell(c, 3).glyph, H, "bottom edge at {c}");
        }
        for r in 1..3 {
            assert_eq!(s.cell(0, r).glyph, V, "left edge at {r}");
            assert_eq!(s.cell(5, r).glyph, V, "right edge at {r}");
        }
    }

    #[test]
    fn a_box_interior_is_blanked() {
        let mut s = Screen::new(Mode::Text80x25);
        s.clear(7, 1);
        s.put_str(3, 3, "XXXX", 7, 1);
        draw_box(&mut s, 2, 2, 10, 5, 15, 1);
        assert_eq!(s.cell(3, 3).glyph, 0x20, "the box covers what was beneath");
    }

    #[test]
    fn a_degenerate_box_draws_nothing_rather_than_panicking() {
        let mut s = Screen::new(Mode::Text80x25);
        s.clear(7, 1);
        draw_box(&mut s, 0, 0, 1, 1, 15, 1);
        draw_box(&mut s, 78, 24, 10, 10, 15, 1); // runs off the edge
        assert_eq!(s.cell(0, 0).glyph, 0x20);
    }

    #[test]
    fn a_centered_message_shows_its_title_and_body() {
        let mut s = Screen::new(Mode::Text80x25);
        s.clear(7, 1);
        draw_centered(&mut s, "Error", "File not found", 15, 4);
        let all: String = (0..s.rows()).map(|r| row_text(&s, r)).collect();
        assert!(all.contains("Error"), "title missing");
        assert!(all.contains("File not found"), "body missing");
    }

    #[test]
    fn a_multi_line_body_grows_the_box() {
        let mut s = Screen::new(Mode::Text80x25);
        s.clear(7, 1);
        draw_centered(&mut s, "Help", "one\ntwo\nthree", 15, 1);
        let all: String = (0..s.rows()).map(|r| row_text(&s, r)).collect();
        for line in ["one", "two", "three"] {
            assert!(all.contains(line), "{line} missing");
        }
    }
}
