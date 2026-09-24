//! Application state: the document, where it is on screen, and the
//! interpretation of every command.

use std::path::PathBuf;

use crate::cp437;
use crate::editor::Editor;
use crate::fileio::TAB_STOP;
use crate::input::{Input, Purpose};
use crate::keymap::{Command, Motion};
use crate::menu::{self, MenuState};
use crate::overlay::{self, Overlay, Prompt};
use crate::print::{self, Request, Scope};
use crate::status;
use crate::vga::{Mode, Screen};
use crate::wrap::{self, VisualLine};

/// Body text and status line colours (Global Constraints).
const FG: u8 = 7;
/// The screen's background. The shell also paints the letterbox slack
/// around the picture in it, so the blue runs to the window's edge.
pub const BG: u8 = 1;
const STATUS_FG: u8 = 15;
/// Columns of clearance at each end of the status line.
const STATUS_MARGIN: usize = 2;

/// The cursor holds steady this long after the last keystroke, so it
/// never flickers while you are actually writing.
const CURSOR_STEADY_MS: u64 = 1_000;
/// Half-period of the idle blink: about 1.2Hz, slow enough to read as
/// breathing rather than flashing.
const CURSOR_BLINK_MS: u64 = 420;

/// Shown by F1. The keys stay discoverable without a permanent hint bar
/// eating a row of the writing surface forever.
const HELP_TEXT: &str = "\
F1 or Esc  Menu bar        Alt-=  Menu bar

Cmd-N  New            Cmd-Z  Undo
Cmd-O  Open           Cmd-Shift-Z  Redo
Cmd-S  Save           Cmd-A  Select all
Cmd-Shift-S  Save as  Cmd-C / X / V  Copy, cut, paste
Cmd-Q  Quit           Cmd-P  Print

Opt-Arrow  By word    Cmd-Arrow  Line or document
F3  CRT effects       F5  80x25 / 80x50
F6  Word count        F11  Fullscreen
F7  Exit              F10  Save As
Shft-F7  Print        Shft-F10  Retrieve
Shft-F1  This help

Esc  Close this";

pub struct App {
    editor: Editor,
    screen: Screen,
    path: Option<PathBuf>,
    crlf: bool,
    /// First visual line shown in the viewport.
    viewport_top: usize,
    /// Cached wrap of the whole document, rebuilt when the text changes.
    lines: Vec<VisualLine>,
    /// Column the cursor is trying to keep while moving vertically.
    goal_col: Option<usize>,
    /// Last clock reading handed to `paint_at`.
    now_ms: u64,
    /// When the user last did anything, for the blink hold-off.
    last_input_ms: u64,
    /// The modal box, if one is open. While it is, keystrokes never
    /// reach the document.
    overlay: Overlay,
    /// The word count replaces the status readout until this moment.
    word_count_until: u64,
    /// A value the user entered in a modal, waiting for the shell to act
    /// on it -- naming, saving, and retrieving all touch the filesystem.
    submitted: Option<(Purpose, String)>,
    /// A command the menu fired that only the shell can carry out.
    deferred: Option<Command>,
    /// What the print screen asked for, waiting for the shell.
    print_request: Option<Request>,
    /// A short readout that borrows the status line's right field.
    notice: Option<(String, u64)>,
    /// The status line can be dismissed; its row then holds text.
    status_line: bool,
    /// CRT effects and fullscreen live here so the shell can read them
    /// back; both persist to the config file.
    effects: bool,
    fullscreen: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let editor = Editor::new();
        let lines = wrap::wrap(editor.text(), wrap::TEXT_COLS);
        Self {
            editor,
            screen: Screen::new(Mode::Text80x25),
            path: None,
            crlf: false,
            viewport_top: 0,
            lines,
            goal_col: None,
            now_ms: 0,
            last_input_ms: 0,
            overlay: Overlay::None,
            word_count_until: 0,
            submitted: None,
            deferred: None,
            print_request: None,
            notice: None,
            status_line: true,
            effects: true,
            fullscreen: false,
            should_quit: false,
        }
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn viewport_top(&self) -> usize {
        self.viewport_top
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    pub fn crlf(&self) -> bool {
        self.crlf
    }

    pub fn set_path(&mut self, path: PathBuf, crlf: bool) {
        self.path = Some(path);
        self.crlf = crlf;
    }

    pub fn mark_saved(&mut self) {
        self.editor.mark_saved();
    }

    /// Rows of the grid available for text: everything but the status line.
    pub fn text_rows(&self) -> usize {
        let status = usize::from(self.status_line);
        self.screen.rows() - status - wrap::TEXT_TOP
    }

    /// Replace the document, as when opening a file.
    pub fn load_text(&mut self, text: &str) {
        self.editor = Editor::from_str(text);
        self.viewport_top = 0;
        self.goal_col = None;
        self.reflow();
    }

    fn reflow(&mut self) {
        self.lines = wrap::wrap(self.editor.text(), wrap::TEXT_COLS);
    }

    fn cursor_position(&self) -> (usize, usize) {
        wrap::position_of(&self.lines, self.editor.cursor())
    }

    /// Scroll the minimum distance that puts the cursor back on screen.
    fn scroll_to_cursor(&mut self) {
        let (line, _) = self.cursor_position();
        let rows = self.text_rows();
        if line < self.viewport_top {
            self.viewport_top = line;
        } else if line >= self.viewport_top + rows {
            self.viewport_top = line + 1 - rows;
        }
    }

    /// Every text-changing command ends the same way.
    fn after_edit(&mut self) {
        self.goal_col = None;
        self.reflow();
        self.scroll_to_cursor();
    }

    fn move_cursor(&mut self, motion: Motion, extend: bool) {
        let (line, col) = self.cursor_position();
        let rows = self.text_rows();
        let cursor = self.editor.cursor();

        // Vertical motion remembers the column it started from, so a trip
        // through a short line does not lose your place.
        let vertical = matches!(
            motion,
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown
        );
        let goal = if vertical {
            let g = self.goal_col.unwrap_or(col);
            self.goal_col = Some(g);
            g
        } else {
            self.goal_col = None;
            col
        };

        let target = match motion {
            Motion::Left => cursor.saturating_sub(1),
            Motion::Right => (cursor + 1).min(self.editor.len()),
            Motion::WordLeft => self.editor.word_left(),
            Motion::WordRight => self.editor.word_right(),
            Motion::LineStart => wrap::offset_at(&self.lines, line, 0),
            Motion::LineEnd => wrap::offset_at(&self.lines, line, usize::MAX),
            Motion::DocStart => 0,
            Motion::DocEnd => self.editor.len(),
            Motion::Up => wrap::offset_at(&self.lines, line.saturating_sub(1), goal),
            Motion::Down => wrap::offset_at(&self.lines, line + 1, goal),
            Motion::PageUp => wrap::offset_at(&self.lines, line.saturating_sub(rows), goal),
            Motion::PageDown => wrap::offset_at(&self.lines, line + rows, goal),
        };

        self.editor.set_cursor(target, extend);
        // set_cursor cleared the run; restore the goal for vertical moves.
        if vertical {
            self.goal_col = Some(goal);
        }
        self.scroll_to_cursor();
    }

    pub fn effects(&self) -> bool {
        self.effects
    }

    pub fn set_effects(&mut self, on: bool) {
        self.effects = on;
    }

    pub fn status_line(&self) -> bool {
        self.status_line
    }

    pub fn set_status_line(&mut self, on: bool) {
        self.status_line = on;
        self.scroll_to_cursor();
    }

    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
    }

    pub fn dense(&self) -> bool {
        self.screen.mode() == Mode::Text80x50
    }

    pub fn set_dense(&mut self, dense: bool) {
        let mode = if dense {
            Mode::Text80x50
        } else {
            Mode::Text80x25
        };
        self.screen.set_mode(mode);
        self.scroll_to_cursor();
    }

    #[cfg(test)]
    pub fn cursor_position_for_test(&self) -> (usize, usize) {
        self.cursor_position()
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub fn set_overlay(&mut self, o: Overlay) {
        self.overlay = o;
    }

    pub fn confirm(&mut self, prompt: Prompt, body: &str) {
        self.overlay = Overlay::Confirm {
            prompt,
            body: body.to_string(),
        };
    }

    /// Answer the open confirmation. The caller performs the resulting
    /// action, since saving and quitting touch the outside world.
    pub fn answer(&mut self, yes: bool) -> Option<(Prompt, bool)> {
        let Overlay::Confirm { prompt, .. } = &self.overlay else {
            return None;
        };
        let prompt = *prompt;
        self.overlay = Overlay::None;
        Some((prompt, yes))
    }

    /// Take whatever a modal field submitted, if anything.
    pub fn take_submitted(&mut self) -> Option<(Purpose, String)> {
        self.submitted.take()
    }

    /// A menu item can fire a command the shell owns -- Save, Open,
    /// Copy -- which `apply` cannot perform. It is parked here for the
    /// shell to pick up after every `apply`.
    pub fn take_deferred(&mut self) -> Option<Command> {
        self.deferred.take()
    }

    pub fn take_print_request(&mut self) -> Option<Request> {
        self.print_request.take()
    }

    /// Show `text` in the status line's right field for three seconds.
    pub fn notify(&mut self, text: &str) {
        self.notice = Some((text.to_string(), self.now_ms + 3_000));
    }

    /// The print screen. The shell passes the printer, since asking the
    /// system for it is its job.
    pub fn open_print(&mut self, printer: Option<String>) {
        self.overlay = Overlay::Print { printer };
    }

    pub fn page_count(&self) -> usize {
        print::page_count(&self.lines)
    }

    /// The print job text for `scope`, or `None` if there is nothing to
    /// print or no such page.
    pub fn print_job(&self, scope: Scope) -> Option<String> {
        if self.editor.to_string().trim().is_empty() {
            return None;
        }
        print::job(self.editor.text(), &self.lines, scope)
    }

    /// Keys reaching the print screen: a digit picks, 0 or Esc leaves.
    fn print_key(&mut self, cmd: Command) {
        let choice = match cmd {
            Command::Insert(text) => text.chars().next(),
            Command::Dismiss | Command::Newline => Some('0'),
            _ => None,
        };
        let (line, _) = self.cursor_position();
        let page = Scope::Page(print::page_of(line));
        match choice {
            Some('1') => self.print_request = Some(Request::Printer(Scope::Full)),
            Some('2') => self.print_request = Some(Request::Printer(page)),
            Some('3') => self.print_request = Some(Request::ToFile(Scope::Full)),
            Some('0') => {}
            _ => return,
        }
        self.overlay = Overlay::None;
    }

    pub fn open_menu(&mut self) {
        self.overlay = Overlay::Menu(MenuState::new());
    }

    pub fn open_field(&mut self, purpose: Purpose, title: &str, label: &str, seed: &str) {
        let mut field = Input::new(purpose, title, label);
        field.set_value(seed);
        self.overlay = Overlay::Field(field);
    }

    /// Keys reaching an open menu. Returns a command it fired, if any.
    fn menu_key(&mut self, cmd: Command) -> Option<Command> {
        let Overlay::Menu(state) = &mut self.overlay else {
            return None;
        };
        let fired = match cmd {
            Command::Move {
                motion: Motion::Left,
                ..
            } => {
                state.left();
                None
            }
            Command::Move {
                motion: Motion::Right,
                ..
            } => {
                state.right();
                None
            }
            Command::Move {
                motion: Motion::Up, ..
            } => {
                state.up();
                None
            }
            Command::Move {
                motion: Motion::Down,
                ..
            } => {
                state.down();
                None
            }
            Command::Newline => state.activate(),
            Command::Insert(text) => text.chars().next().and_then(|c| state.letter(c)),
            Command::Dismiss => {
                if !state.escape() {
                    self.overlay = Overlay::None;
                }
                None
            }
            // Anything else closes the menu and is discarded.
            _ => {
                self.overlay = Overlay::None;
                None
            }
        };
        if fired.is_some() {
            self.overlay = Overlay::None;
        }
        fired
    }

    /// Keys reaching an open modal field.
    fn field_key(&mut self, cmd: Command) {
        let Overlay::Field(field) = &mut self.overlay else {
            return;
        };
        match cmd {
            Command::Insert(text) => field.insert(&text),
            Command::Backspace => field.backspace(),
            Command::DeleteForward => field.delete(),
            Command::Move {
                motion: Motion::Left,
                ..
            } => field.left(),
            Command::Move {
                motion: Motion::Right,
                ..
            } => field.right(),
            Command::Move {
                motion: Motion::LineStart,
                ..
            } => field.home(),
            Command::Move {
                motion: Motion::LineEnd,
                ..
            } => field.end(),
            Command::Newline => {
                let value = field.value();
                let purpose = field.purpose();
                self.overlay = Overlay::None;
                // An empty name is the same as backing out.
                if !value.trim().is_empty() {
                    self.submitted = Some((purpose, value));
                }
            }
            Command::Dismiss => self.overlay = Overlay::None,
            _ => {}
        }
    }

    /// Solid while typing; a slow blink once you pause.
    pub fn cursor_visible(&self, now_ms: u64) -> bool {
        let since = now_ms.saturating_sub(self.last_input_ms);
        if since < CURSOR_STEADY_MS {
            return true;
        }
        ((since - CURSOR_STEADY_MS) / CURSOR_BLINK_MS).is_multiple_of(2)
    }

    pub fn apply(&mut self, cmd: Command, now_ms: u64) {
        self.last_input_ms = now_ms;
        // Whatever holds focus consumes the key first.
        match &self.overlay {
            Overlay::Menu(_) => {
                if let Some(fired) = self.menu_key(cmd) {
                    self.apply(fired, now_ms);
                }
                return;
            }
            Overlay::Field(_) => {
                self.field_key(cmd);
                return;
            }
            Overlay::Print { .. } => {
                self.print_key(cmd);
                return;
            }
            Overlay::Message { .. } | Overlay::Confirm { .. } => {
                if matches!(cmd, Command::Dismiss) {
                    self.overlay = Overlay::None;
                }
                return;
            }
            Overlay::None => {}
        }
        match cmd {
            Command::Insert(text) => {
                // Filter through CP437 so the buffer can only ever hold
                // characters the screen can draw.
                let filtered: String = text.chars().filter_map(cp437::accept).collect();
                if filtered.is_empty() {
                    return;
                }
                self.editor.insert(&filtered, now_ms);
                self.after_edit();
            }
            Command::Newline => {
                self.editor.insert("\n", now_ms);
                self.after_edit();
            }
            Command::Tab => {
                let (_, col) = self.cursor_position();
                let spaces = TAB_STOP - (col % TAB_STOP);
                self.editor.insert(&" ".repeat(spaces), now_ms);
                self.after_edit();
            }
            Command::Backspace => {
                self.editor.backspace(now_ms);
                self.after_edit();
            }
            Command::DeleteForward => {
                self.editor.delete_forward(now_ms);
                self.after_edit();
            }
            Command::Move { motion, extend } => self.move_cursor(motion, extend),
            Command::SelectAll => {
                self.editor.set_cursor(0, false);
                self.editor.set_cursor(self.editor.len(), true);
            }
            Command::Undo => {
                if self.editor.undo() {
                    self.after_edit();
                }
            }
            Command::Redo => {
                if self.editor.redo() {
                    self.after_edit();
                }
            }
            // Nothing to dismiss when the document has focus, so Esc
            // becomes the always-available way to reach the menu.
            Command::MenuBar | Command::Dismiss => self.open_menu(),
            Command::Quit => {
                if self.editor.is_dirty() {
                    self.confirm(Prompt::QuitUnsaved, "Save changes to this document? (Y/N)");
                } else {
                    self.should_quit = true;
                }
            }
            Command::ToggleHelp => {
                self.overlay = Overlay::Message {
                    title: "Help".to_string(),
                    body: HELP_TEXT.to_string(),
                    danger: false,
                };
            }
            Command::ShowWordCount => self.word_count_until = now_ms + 3_000,
            Command::ToggleDenseMode => {
                let dense = self.dense();
                self.set_dense(!dense);
            }
            Command::ToggleEffects => self.effects = !self.effects,
            Command::ToggleStatusLine => self.set_status_line(!self.status_line),
            Command::ToggleFullscreen => self.fullscreen = !self.fullscreen,
            // The shell performs these: they touch the OS. A hotkey
            // never gets here (the shell intercepts it first), so this
            // is a menu item firing, and the shell collects it after.
            Command::Copy
            | Command::Cut
            | Command::Paste
            | Command::New
            | Command::Open
            | Command::Retrieve
            | Command::Save
            | Command::SaveAs
            | Command::Print => self.deferred = Some(cmd),
        }
    }

    /// Test hook: the wrap cache, so tests can compute expected offsets.
    #[cfg(test)]
    pub fn lines_for_test(&self) -> &[VisualLine] {
        &self.lines
    }

    /// The character offset under a grid cell.
    pub fn offset_at_cell(&self, col: usize, row: usize) -> usize {
        let line = self.viewport_top + row.saturating_sub(wrap::TEXT_TOP);
        // Clicking in the empty space below the text lands at the end of
        // the document, not at the start of the last line.
        if line >= self.lines.len() {
            return self.editor.len();
        }
        let col = col.saturating_sub(wrap::TEXT_LEFT);
        wrap::offset_at(&self.lines, line, col)
    }

    pub fn click(&mut self, col: usize, row: usize, extend: bool) {
        self.last_input_ms = self.now_ms;
        let offset = self.offset_at_cell(col, row);
        self.editor.set_cursor(offset, extend);
        self.goal_col = None;
        self.scroll_to_cursor();
    }

    /// Scroll without moving the caret, as a scroll wheel should.
    pub fn scroll(&mut self, lines: i32) {
        let max_top = self.lines.len().saturating_sub(self.text_rows());
        let next = self.viewport_top as i64 + lines as i64;
        self.viewport_top = next.clamp(0, max_top as i64) as usize;
    }

    /// Repaint the grid from application state. `blink_on` drives the
    /// cursor's 2Hz blink.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn paint(&mut self) {
        self.paint_at(0)
    }

    /// `now_ms` drives the cursor blink and the word-count expiry.
    pub fn paint_at(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        let blink_on = self.cursor_visible(now_ms);
        self.screen.clear(FG, BG);
        let rows = self.text_rows();
        let text = self.editor.text();

        for row in 0..rows {
            let Some(line) = self.lines.get(self.viewport_top + row) else {
                break;
            };
            let s: String = text[line.start..line.end].iter().collect();
            self.screen
                .put_str(wrap::TEXT_LEFT, wrap::TEXT_TOP + row, &s, FG, BG);
        }

        self.paint_selection();
        self.screen.set_cursor(None);
        // An open modal owns the cursor. Blinking it in the document behind
        // the box points at the one place the typing is not going.
        if blink_on && self.editor.selection().is_none() && !self.overlay_owns_cursor() {
            self.paint_cursor();
        }
        self.paint_status();
        self.paint_overlay(blink_on);
    }

    /// Whether the open overlay places the cursor itself.
    fn overlay_owns_cursor(&self) -> bool {
        matches!(self.overlay, Overlay::Field(_) | Overlay::Print { .. })
    }

    /// WordPerfect's Shift-F7 screen: the document goes away, the
    /// choices sit at the top left, and "Selection: 0" waits at the
    /// bottom with the cursor on the 0.
    fn paint_print(&mut self, printer: Option<&str>) {
        self.screen.clear(FG, BG);
        let rows = self.screen.rows();
        let (line, _) = self.cursor_position();
        let here = print::page_of(line) + 1;
        let pages = self.page_count();

        self.screen.put_str(0, 0, "Print", FG, BG);
        self.screen.put_str(5, 2, "1 - Full Document", FG, BG);
        self.screen.put_str(5, 3, "2 - Page", FG, BG);
        self.screen.put_str(5, 4, "3 - Document to Disk", FG, BG);

        self.screen.put_str(0, 6, "Options", FG, BG);
        let printer = printer.unwrap_or("(none selected)");
        let printer_line = format!("Printer               {printer}");
        self.screen.put_str(5, 8, &printer_line, FG, BG);
        let pages_line = format!("Pages                 {pages}   (cursor on page {here})");
        self.screen.put_str(5, 9, &pages_line, FG, BG);

        let sel = "Selection: 0";
        let bottom = rows - 1;
        self.screen.put_str(0, bottom, sel, FG, BG);
        self.screen
            .set_cursor(Some((sel.chars().count() - 1, bottom)));
    }

    fn paint_overlay(&mut self, blink_on: bool) {
        match self.overlay.clone() {
            Overlay::None => {}
            Overlay::Menu(state) => self.paint_menu(&state),
            Overlay::Field(field) => self.paint_field(&field, blink_on),
            Overlay::Message {
                title,
                body,
                danger,
            } => {
                let bg = if danger { 4 } else { BG };
                overlay::draw_centered(&mut self.screen, &title, &body, 15, bg);
            }
            Overlay::Confirm { body, .. } => {
                overlay::draw_centered(&mut self.screen, "", &body, 15, 1);
            }
            Overlay::Print { printer } => self.paint_print(printer.as_deref()),
        }
    }

    fn paint_selection(&mut self) {
        let Some((lo, hi)) = self.editor.selection() else {
            return;
        };
        let rows = self.text_rows();
        for row in 0..rows {
            let Some(line) = self.lines.get(self.viewport_top + row).copied() else {
                break;
            };
            let from = lo.max(line.start);
            let to = hi.min(line.end);
            for offset in from..to {
                let col = wrap::TEXT_LEFT + (offset - line.start);
                self.screen.invert(col, wrap::TEXT_TOP + row);
            }
        }
    }

    fn paint_cursor(&mut self) {
        let (line, col) = self.cursor_position();
        if line < self.viewport_top {
            return;
        }
        let row = line - self.viewport_top;
        if row < self.text_rows() {
            self.screen
                .set_cursor(Some((wrap::TEXT_LEFT + col, wrap::TEXT_TOP + row)));
        }
    }

    /// The bar across row 0, plus a dropdown when one is pulled down.
    fn paint_menu(&mut self, state: &MenuState) {
        let bar_fg = 0; // black on grey: the bar is a surface, not text
        let bar_bg = 7;
        for col in 0..self.screen.cols() {
            self.screen.set(col, 0, 0x20, bar_fg, bar_bg);
        }

        let mut x = 2;
        let mut open_at = 0;
        for (i, m) in menu::MENUS.iter().enumerate() {
            let selected = i == state.menu();
            let (fg, bg) = if selected {
                (bar_bg, bar_fg)
            } else {
                (bar_fg, bar_bg)
            };
            self.screen.put_str(x, 0, " ", fg, bg);
            self.screen.put_str(x + 1, 0, m.title, fg, bg);
            self.screen
                .put_str(x + 1 + m.title.chars().count(), 0, " ", fg, bg);
            if selected {
                open_at = x;
            }
            x += m.title.chars().count() + 3;
        }

        let Some(highlighted) = state.item() else {
            return;
        };
        let items = menu::MENUS[state.menu()].items;
        let widest = items
            .iter()
            .map(|i| i.label.chars().count() + i.hotkey.chars().count() + 6)
            .max()
            .unwrap_or(20);
        let width = widest.clamp(20, self.screen.cols() - open_at);
        let height = items.len() + 2;

        overlay::draw_box(&mut self.screen, open_at, 1, width, height, bar_fg, bar_bg);
        for (i, it) in items.iter().enumerate() {
            let row = 2 + i;
            if it.command.is_none() {
                // A rule across the interior, tied into both edges.
                for c in 1..width - 1 {
                    self.screen.set(open_at + c, row, 0xC4, bar_fg, bar_bg);
                }
                self.screen.set(open_at, row, 0xC7, bar_fg, bar_bg);
                self.screen
                    .set(open_at + width - 1, row, 0xB6, bar_fg, bar_bg);
                continue;
            }
            let selected = i == highlighted;
            let (fg, bg) = if selected {
                (bar_bg, bar_fg)
            } else {
                (bar_fg, bar_bg)
            };
            for c in 1..width - 1 {
                self.screen.set(open_at + c, row, 0x20, fg, bg);
            }
            self.screen.put_str(open_at + 2, row, it.label, fg, bg);
            if !it.hotkey.is_empty() {
                let hx = open_at + width - 2 - it.hotkey.chars().count();
                self.screen.put_str(hx, row, it.hotkey, fg, bg);
            }
        }
    }

    /// A framed box with a single editable line and a block cursor.
    fn paint_field(&mut self, field: &Input, blink_on: bool) {
        let fg = 15;
        let bg = BG;
        let inner = 44usize;
        let width = inner + 6;
        let height = 7;
        let x = (self.screen.cols() - width) / 2;
        let y = (self.screen.rows().saturating_sub(height)) / 2;

        overlay::draw_box(&mut self.screen, x, y, width, height, fg, bg);
        let title = format!(" {} ", field.title());
        let tx = x + (width.saturating_sub(title.chars().count())) / 2;
        self.screen.put_str(tx, y, &title, fg, bg);
        self.screen.put_str(x + 3, y + 2, field.label(), fg, bg);

        // The field itself, sunk into the box.
        let fx = x + 3;
        let fy = y + 3;
        for c in 0..inner {
            self.screen.set(fx + c, fy, 0x20, 0, 7);
        }
        // Scroll the value so the cursor stays visible in a long path.
        let value: Vec<char> = field.value().chars().collect();
        let offset = field.cursor().saturating_sub(inner.saturating_sub(1));
        let shown: String = value.iter().skip(offset).take(inner).collect();
        self.screen.put_str(fx, fy, &shown, 0, 7);
        // The same underline cursor the document uses, blinking in step with
        // it, so the caret is wherever the next keystroke will land.
        let cx = fx + (field.cursor() - offset).min(inner - 1);
        if blink_on {
            self.screen.set_cursor(Some((cx, fy)));
        }

        let help = "Enter  Accept     Esc  Cancel";
        let hx = x + (width.saturating_sub(help.chars().count())) / 2;
        self.screen.put_str(hx, y + 5, help, fg, bg);
    }

    fn paint_status(&mut self) {
        if !self.status_line {
            return;
        }
        let row = self.screen.rows() - 1;
        // The status line is its own colour across the full width.
        for col in 0..self.screen.cols() {
            self.screen.set(col, row, 0x20, STATUS_FG, BG);
        }

        // A column of margin each side: the tube's curvature eats the very
        // edge of the screen, and text flush to it reads as clipped.
        let left = status::dos_path(self.path.as_deref(), self.editor.is_dirty());
        self.screen
            .put_str(STATUS_MARGIN, row, &left, STATUS_FG, BG);

        let notice = match &self.notice {
            Some((text, until)) if self.now_ms < *until => Some(text.clone()),
            _ => None,
        };
        let right = if let Some(text) = notice {
            text
        } else if self.now_ms < self.word_count_until {
            format!("{} words", self.editor.word_count())
        } else {
            let (line, col) = self.cursor_position();
            status::right_field(&status::measure(line, col))
        };
        let start = self
            .screen
            .cols()
            .saturating_sub(right.chars().count() + STATUS_MARGIN);
        self.screen.put_str(start, row, &right, STATUS_FG, BG);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 0;

    fn app_with(text: &str) -> App {
        let mut a = App::new();
        a.load_text(text);
        a
    }

    #[test]
    fn a_new_app_starts_empty_at_the_top() {
        let a = App::new();
        assert_eq!(a.viewport_top(), 0);
        assert_eq!(a.editor().cursor(), 0);
    }

    #[test]
    fn typing_inserts_and_marks_the_document_dirty() {
        let mut a = App::new();
        a.apply(Command::Insert("hi".to_string()), T);
        assert_eq!(a.editor().to_string(), "hi");
        assert!(a.editor().is_dirty());
    }

    #[test]
    fn unrepresentable_characters_are_dropped_at_the_boundary() {
        let mut a = App::new();
        a.apply(Command::Insert("a\u{3042}b".to_string()), T);
        assert_eq!(a.editor().to_string(), "ab", "no glyph, no character");
    }

    #[test]
    fn typographic_characters_are_substituted_on_the_way_in() {
        let mut a = App::new();
        a.apply(Command::Insert("\u{201C}x\u{201D}".to_string()), T);
        assert_eq!(a.editor().to_string(), "\"x\"");
    }

    #[test]
    fn tab_inserts_spaces_to_the_next_stop() {
        let mut a = App::new();
        a.apply(Command::Tab, T);
        assert_eq!(a.editor().to_string(), "        ");
        a.apply(Command::Insert("x".to_string()), T);
        a.apply(Command::Tab, T);
        assert_eq!(
            a.editor().to_string().len(),
            16,
            "next stop, not another eight"
        );
    }

    #[test]
    fn moving_down_a_line_keeps_the_column() {
        let mut a = app_with("abcdef\nghijkl");
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
            T,
        );
        assert_eq!(a.editor().cursor(), 9, "column 2 of the second line");
    }

    #[test]
    fn moving_down_onto_a_shorter_line_clamps_to_its_end() {
        let mut a = app_with("abcdef\nxy");
        a.apply(
            Command::Move {
                motion: Motion::LineEnd,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
            T,
        );
        assert_eq!(a.editor().cursor(), 9, "end of the short line");
    }

    #[test]
    fn the_goal_column_survives_a_short_line() {
        let mut a = app_with("abcdef\nxy\nabcdef");
        a.apply(
            Command::Move {
                motion: Motion::LineEnd,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
            T,
        );
        assert_eq!(a.editor().cursor(), 16, "column 6 again on the third line");
    }

    #[test]
    fn line_start_and_end_work_on_the_visual_line() {
        let mut a = app_with("ab\ncd");
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::LineStart,
                extend: false,
            },
            T,
        );
        assert_eq!(a.editor().cursor(), 3);
        a.apply(
            Command::Move {
                motion: Motion::LineEnd,
                extend: false,
            },
            T,
        );
        assert_eq!(a.editor().cursor(), 5);
    }

    #[test]
    fn shift_movement_builds_a_selection() {
        let mut a = app_with("abcdef");
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: true,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: true,
            },
            T,
        );
        assert_eq!(a.editor().selection(), Some((0, 2)));
    }

    #[test]
    fn select_all_covers_the_document() {
        let mut a = app_with("abc");
        a.apply(Command::SelectAll, T);
        assert_eq!(a.editor().selection(), Some((0, 3)));
    }

    #[test]
    fn the_viewport_follows_the_cursor_down() {
        // 40 lines in a 24-row viewport.
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = app_with(&text);
        assert_eq!(a.viewport_top(), 0);
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        let rows = a.text_rows();
        assert_eq!(
            a.viewport_top(),
            40 - rows,
            "the last line is the bottom one"
        );
    }

    #[test]
    fn the_viewport_follows_the_cursor_back_up() {
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = app_with(&text);
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        a.apply(
            Command::Move {
                motion: Motion::DocStart,
                extend: false,
            },
            T,
        );
        assert_eq!(a.viewport_top(), 0);
    }

    #[test]
    fn a_short_document_never_scrolls() {
        let mut a = app_with("one\ntwo");
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        assert_eq!(a.viewport_top(), 0);
    }

    #[test]
    fn painting_puts_text_at_the_left_margin() {
        let mut a = app_with("hello");
        a.paint();
        assert_eq!(a.screen().cell(wrap::TEXT_LEFT, wrap::TEXT_TOP).glyph, b'h');
        assert_eq!(
            a.screen().cell(wrap::TEXT_LEFT - 1, wrap::TEXT_TOP).glyph,
            0x20,
            "margin is blank"
        );
    }

    #[test]
    fn painting_leaves_a_blank_row_above_the_text() {
        let mut a = app_with("hello");
        a.paint();
        for col in 0..a.screen().cols() {
            assert_eq!(a.screen().cell(col, 0).glyph, 0x20, "row 0 is margin");
        }
        assert_eq!(
            a.editor().to_string(),
            "hello",
            "the margin is not in the buffer"
        );
    }

    #[test]
    fn the_menu_bar_does_not_cover_the_cursor() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        a.paint_at(T);
        let (_, row) = a.screen().cursor().expect("cursor still placed");
        assert!(row > 0, "cursor sits below the bar on row 0");
    }

    #[test]
    fn painting_places_the_cursor_at_the_caret() {
        let mut a = app_with("hi");
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: false,
            },
            T,
        );
        a.paint_at(T);
        assert_eq!(
            a.screen().cursor(),
            Some((wrap::TEXT_LEFT + 1, wrap::TEXT_TOP))
        );
    }

    #[test]
    fn an_open_field_takes_the_cursor_from_the_document() {
        let mut a = app_with("It was a bright cold day in April");
        a.paint_at(0);
        let in_document = a.screen().cursor().expect("a cursor in the document");

        a.open_field(
            Purpose::CreateAtLaunch,
            "New Document",
            "Document to be created:",
            "",
        );
        a.paint_at(0);
        let in_field = a.screen().cursor().expect("a cursor in the field");

        assert_ne!(in_field, in_document, "the caret follows the modal");
        assert!(
            in_field.1 > wrap::TEXT_TOP,
            "the caret is down in the box, not on a text row: {in_field:?}"
        );
    }

    #[test]
    fn dismissing_the_status_line_gives_its_row_to_the_text() {
        let mut a = app_with("hello");
        let rows_with = a.text_rows();
        let bottom = a.screen().rows() - 1;

        a.paint_at(0);
        assert_eq!(
            a.screen().cell(0, bottom).fg,
            STATUS_FG,
            "the status line is there to begin with"
        );

        a.apply(Command::ToggleStatusLine, T);
        assert!(!a.status_line());
        assert_eq!(a.text_rows(), rows_with + 1, "the text gains the freed row");
        a.paint_at(T);
        assert_ne!(
            a.screen().cell(0, bottom).fg,
            STATUS_FG,
            "the bottom row is no longer the status line"
        );

        a.apply(Command::ToggleStatusLine, T);
        assert!(a.status_line(), "and it comes back");
        assert_eq!(a.text_rows(), rows_with);
    }

    #[test]
    fn the_cursor_hides_while_a_selection_is_active() {
        let mut a = app_with("hello");
        a.apply(Command::SelectAll, T);
        a.paint_at(T);
        assert_eq!(a.screen().cursor(), None, "the selection is the marker");
    }

    #[test]
    fn the_cursor_holds_steady_while_typing() {
        let mut a = App::new();
        // Type across more than a blink period; it must never wink out.
        for step in 0..6u64 {
            a.apply(Command::Insert("a".to_string()), step * 200);
            assert!(a.cursor_visible(step * 200), "blinked mid-keystroke");
            assert!(
                a.cursor_visible(step * 200 + 150),
                "blinked between keystrokes"
            );
        }
    }

    #[test]
    fn the_cursor_stays_lit_for_a_moment_after_you_stop() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), 1_000);
        assert!(
            a.cursor_visible(1_000 + CURSOR_STEADY_MS - 1),
            "still steady"
        );
    }

    #[test]
    fn the_cursor_blinks_once_you_pause() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), 0);
        let base = CURSOR_STEADY_MS;
        assert!(a.cursor_visible(base), "on");
        assert!(!a.cursor_visible(base + CURSOR_BLINK_MS), "off");
        assert!(a.cursor_visible(base + CURSOR_BLINK_MS * 2), "on again");
    }

    #[test]
    fn the_idle_blink_is_slow_enough_to_read_as_breathing() {
        // Measure the real thing rather than the constant: sample the
        // cursor once a millisecond and time one complete cycle.
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), 0);

        let start = CURSOR_STEADY_MS + 1;
        let mut transitions = Vec::new();
        let mut previous = a.cursor_visible(start);
        for t in start..start + 5_000 {
            let now = a.cursor_visible(t);
            if now != previous {
                transitions.push(t);
                previous = now;
            }
            if transitions.len() == 3 {
                break;
            }
        }
        assert_eq!(transitions.len(), 3, "expected the cursor to blink at all");
        let cycle = transitions[2] - transitions[0];
        assert!(
            cycle >= 700,
            "a {cycle}ms cycle reads as flashing, not breathing"
        );
        assert!(
            cycle <= 1_600,
            "a {cycle}ms cycle is so slow it looks broken"
        );
    }

    #[test]
    fn moving_the_caret_also_holds_the_cursor_steady() {
        let mut a = app_with("hello");
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: false,
            },
            5_000,
        );
        assert!(
            a.cursor_visible(5_000 + 100),
            "arrowing about should not blink"
        );
    }

    #[test]
    fn clicking_holds_the_cursor_steady_too() {
        let mut a = app_with("hello");
        a.paint_at(9_000);
        a.click(wrap::TEXT_LEFT + 2, wrap::TEXT_TOP, false);
        assert!(a.cursor_visible(9_000 + 100));
    }

    #[test]
    fn painting_writes_the_status_line_on_the_last_row() {
        let mut a = app_with("hi");
        a.paint();
        let row = a.screen().rows() - 1;
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(line.contains("(UNTITLED)"), "got: {line}");
        assert!(line.contains("Ln 1\""), "got: {line}");
        assert!(line.contains("Pos 1\""), "got: {line}");
    }

    #[test]
    fn the_status_line_reflects_the_cursor_position() {
        let mut a = app_with("abcde");
        a.apply(
            Command::Move {
                motion: Motion::LineEnd,
                extend: false,
            },
            T,
        );
        a.paint();
        let row = a.screen().rows() - 1;
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(line.contains("Pos 1.5\""), "five characters in: {line}");
    }

    #[test]
    fn the_status_line_right_field_is_not_clipped() {
        let mut a = app_with("hi");
        a.paint();
        let row = a.screen().rows() - 1;
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(
            line.trim_end().ends_with("Pos 1\""),
            "right-aligned: {line:?}"
        );
    }

    #[test]
    fn a_click_maps_a_cell_to_a_character_offset() {
        let a = app_with("hello\nworld");
        let top = wrap::TEXT_TOP;
        assert_eq!(a.offset_at_cell(wrap::TEXT_LEFT + 2, top), 2);
        assert_eq!(a.offset_at_cell(wrap::TEXT_LEFT + 3, top + 1), 9);
    }

    #[test]
    fn a_click_in_the_top_margin_lands_on_the_first_line() {
        let a = app_with("hello\nworld");
        assert_eq!(a.offset_at_cell(wrap::TEXT_LEFT + 2, 0), 2);
    }

    #[test]
    fn a_click_left_of_the_margin_lands_at_the_line_start() {
        let a = app_with("hello");
        assert_eq!(a.offset_at_cell(0, 0), 0);
    }

    #[test]
    fn a_click_past_the_end_of_a_line_lands_at_its_end() {
        let a = app_with("hi\nthere");
        assert_eq!(a.offset_at_cell(79, wrap::TEXT_TOP), 2);
    }

    #[test]
    fn a_click_below_the_last_line_lands_at_the_document_end() {
        let a = app_with("hi");
        assert_eq!(a.offset_at_cell(wrap::TEXT_LEFT, 20), 2);
    }

    #[test]
    fn clicking_moves_the_caret_and_clears_the_selection() {
        let mut a = app_with("hello");
        a.apply(Command::SelectAll, T);
        a.click(wrap::TEXT_LEFT + 2, wrap::TEXT_TOP, false);
        assert_eq!(a.editor().cursor(), 2);
        assert_eq!(a.editor().selection(), None);
    }

    #[test]
    fn dragging_extends_the_selection() {
        let mut a = app_with("hello");
        a.click(wrap::TEXT_LEFT + 1, wrap::TEXT_TOP, false);
        a.click(wrap::TEXT_LEFT + 4, wrap::TEXT_TOP, true);
        assert_eq!(a.editor().selection(), Some((1, 4)));
    }

    #[test]
    fn the_offset_accounts_for_scrolling() {
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = app_with(&text);
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        let top = a.viewport_top();
        assert!(top > 0);
        let expected = wrap::offset_at(a.lines_for_test(), top, 0);
        assert_eq!(a.offset_at_cell(wrap::TEXT_LEFT, wrap::TEXT_TOP), expected);
    }

    #[test]
    fn scrolling_does_not_move_the_caret() {
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = app_with(&text);
        a.scroll(5);
        assert_eq!(a.viewport_top(), 5);
        assert_eq!(a.editor().cursor(), 0, "the caret stays put");
    }

    #[test]
    fn scrolling_stops_at_the_document_edges() {
        let mut a = app_with("one\ntwo");
        a.scroll(-10);
        assert_eq!(a.viewport_top(), 0);
        a.scroll(100);
        assert_eq!(a.viewport_top(), 0, "a short document has nowhere to go");
    }

    /// Visual check of the whole paint path. Run deliberately with
    /// `cargo test dump_app_preview -- --ignored`.
    #[test]
    #[ignore]
    fn dump_app_preview() {
        use crate::vga::{FB_HEIGHT, FB_WIDTH};

        let mut a = App::new();
        a.set_path(
            std::path::PathBuf::from("/Users/srhise/Documents/chapter-one.txt"),
            false,
        );
        for (i, word) in "It was a bright cold day in April, and the clocks were \
striking thirteen. Winston Smith, his chin nuzzled into his breast in an effort \
to escape the vile wind, slipped quickly through the glass doors of Victory \
Mansions, though not quickly enough to prevent a swirl of gritty dust from \
entering along with him."
            .split(' ')
            .enumerate()
        {
            if i > 0 {
                a.apply(Command::Insert(" ".to_string()), i as u64 * 10);
            }
            a.apply(Command::Insert(word.to_string()), i as u64 * 10);
        }
        a.apply(Command::Newline, 9_000);
        a.apply(Command::Newline, 9_000);
        a.apply(
            Command::Insert("The hallway smelt of boiled cabbage and old rag mats.".to_string()),
            9_100,
        );

        a.paint();
        let mut fb = vec![0u8; FB_WIDTH * FB_HEIGHT * 4];
        a.screen().render(&mut fb);
        crate::vga::preview::write_bmp("target/app-preview.bmp", &fb);
        println!("wrote target/app-preview.bmp");
    }

    #[test]
    fn quitting_a_clean_document_needs_no_prompt() {
        let mut a = app_with("saved text");
        a.apply(Command::Quit, T);
        assert!(a.should_quit);
        assert!(matches!(a.overlay(), Overlay::None));
    }

    #[test]
    fn quitting_a_dirty_document_asks_first() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), T);
        a.apply(Command::Quit, T);
        assert!(!a.should_quit, "must not exit while a prompt is open");
        assert!(matches!(a.overlay(), Overlay::Confirm { .. }));
    }

    #[test]
    fn answering_reports_the_prompt_and_the_answer() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), T);
        a.apply(Command::Quit, T);
        assert_eq!(a.answer(false), Some((Prompt::QuitUnsaved, false)));
        assert!(matches!(a.overlay(), Overlay::None));
    }

    #[test]
    fn answering_yes_reports_yes() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), T);
        a.apply(Command::Quit, T);
        assert_eq!(a.answer(true), Some((Prompt::QuitUnsaved, true)));
    }

    #[test]
    fn answering_with_no_prompt_open_reports_nothing() {
        let mut a = App::new();
        assert_eq!(a.answer(true), None);
    }

    #[test]
    fn escape_dismisses_a_prompt_without_acting() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), T);
        a.apply(Command::Quit, T);
        a.apply(Command::Dismiss, T);
        assert!(matches!(a.overlay(), Overlay::None));
        assert!(!a.should_quit);
    }

    #[test]
    fn keystrokes_do_not_reach_the_document_while_an_overlay_is_open() {
        let mut a = App::new();
        a.apply(Command::Insert("x".to_string()), T);
        a.apply(Command::Quit, T);
        a.apply(Command::Insert("zzz".to_string()), T);
        a.apply(Command::Backspace, T);
        assert_eq!(a.editor().to_string(), "x", "the prompt swallowed them");
    }

    #[test]
    fn an_error_overlay_renders_a_framed_box() {
        let mut a = App::new();
        a.set_overlay(Overlay::Message {
            title: "Error".to_string(),
            body: "File not found".to_string(),
            danger: true,
        });
        a.paint();
        let has_corner =
            (0..a.screen().rows()).any(|r| (0..80).any(|c| a.screen().cell(c, r).glyph == 0xC9));
        assert!(has_corner, "expected a double-line top-left corner");
    }

    #[test]
    fn f1_opens_and_closes_the_help_overlay() {
        let mut a = App::new();
        a.apply(Command::ToggleHelp, T);
        assert!(matches!(a.overlay(), Overlay::Message { .. }));
        a.apply(Command::Dismiss, T);
        assert!(matches!(a.overlay(), Overlay::None));
    }

    #[test]
    fn the_help_overlay_lists_the_keys() {
        let mut a = App::new();
        a.apply(Command::ToggleHelp, T);
        a.paint();
        let all: String = (0..a.screen().rows())
            .flat_map(|r| (0..80).map(move |c| (c, r)))
            .map(|(c, r)| cp437::decode(a.screen().cell(c, r).glyph))
            .collect();
        assert!(all.contains("Undo"), "expected the key list");
    }

    #[test]
    fn the_word_count_appears_in_the_status_line_then_expires() {
        let mut a = app_with("one two three");
        a.apply(Command::ShowWordCount, T);
        a.paint_at(T);
        let row = a.screen().rows() - 1;
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(line.contains("3 words"), "got: {line}");

        // Three seconds later the status line is back to normal.
        a.paint_at(T + 3_001);
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(!line.contains("3 words"), "got: {line}");
        assert!(line.contains("Pos"), "got: {line}");
    }

    #[test]
    fn dense_mode_doubles_the_available_rows() {
        let mut a = App::new();
        assert_eq!(a.text_rows(), 23);
        a.apply(Command::ToggleDenseMode, T);
        assert_eq!(a.text_rows(), 48);
        assert!(a.dense());
        a.apply(Command::ToggleDenseMode, T);
        assert_eq!(a.text_rows(), 23);
    }

    #[test]
    fn switching_modes_keeps_the_cursor_on_screen() {
        let text = (0..60)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = app_with(&text);
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        a.apply(Command::ToggleDenseMode, T);
        let (line, _) = a.cursor_position_for_test();
        assert!(
            line >= a.viewport_top() && line < a.viewport_top() + a.text_rows(),
            "cursor at line {line} is off screen"
        );
    }

    #[test]
    fn the_effects_and_fullscreen_toggles_flip() {
        let mut a = App::new();
        assert!(a.effects());
        a.apply(Command::ToggleEffects, T);
        assert!(!a.effects());
        assert!(!a.fullscreen());
        a.apply(Command::ToggleFullscreen, T);
        assert!(a.fullscreen());
    }

    /// Visual check of the help overlay. Run with
    /// `cargo test dump_help_preview -- --ignored`.
    #[test]
    #[ignore]
    fn dump_help_preview() {
        use crate::vga::{FB_HEIGHT, FB_WIDTH};

        let mut a = App::new();
        a.set_path(
            std::path::PathBuf::from("/Users/srhise/Documents/chapter-one.txt"),
            false,
        );
        a.load_text(
            "The hallway smelt of boiled cabbage and old rag mats. At one end \
of it a coloured poster, too large for indoor display, had been tacked to \
the wall.",
        );
        a.apply(Command::ToggleHelp, 0);
        a.paint();

        let mut fb = vec![0u8; FB_WIDTH * FB_HEIGHT * 4];
        a.screen().render(&mut fb);
        crate::vga::preview::write_bmp("target/help-preview.bmp", &fb);
        println!("wrote target/help-preview.bmp");
    }

    /// The states the store screenshots show, dumped at framebuffer
    /// resolution so the listing can scale them by a whole number.
    /// `cargo test dump_store_previews -- --ignored`
    #[test]
    #[ignore]
    fn dump_store_previews() {
        use crate::input::Purpose;
        use crate::vga::{FB_HEIGHT, FB_WIDTH};

        let sample = "It was a bright cold day in April, and the clocks were \
striking thirteen. Winston Smith, his chin nuzzled into his breast in an effort \
to escape the vile wind, slipped quickly through the glass doors of Victory \
Mansions, though not quickly enough to prevent a swirl of gritty dust from \
entering along with him.";

        let shot = |name: &str, prepare: &dyn Fn(&mut App)| {
            let mut a = App::new();
            a.set_path(
                std::path::PathBuf::from("/Users/srhise/Documents/chapter-one.txt"),
                false,
            );
            a.load_text(sample);
            prepare(&mut a);
            a.paint();
            let mut fb = vec![0u8; FB_WIDTH * FB_HEIGHT * 4];
            a.screen().render(&mut fb);
            crate::vga::preview::write_bmp(&format!("target/shot-{name}.bmp"), &fb);
            println!("wrote target/shot-{name}.bmp");
        };

        // The bar alone shows no shortcuts; it is the open dropdown that
        // teaches them, which is a Down away.
        shot("menu", &|a| {
            a.apply(Command::MenuBar, 0);
            a.apply(
                Command::Move {
                    motion: crate::keymap::Motion::Down,
                    extend: false,
                },
                0,
            );
        });
        // Printing is raised by the window layer, not by the command.
        shot("print", &|a| a.open_print(Some("HP_LaserJet".to_string())));
        shot("count", &|a| a.apply(Command::ShowWordCount, 0));
        shot("nostatus", &|a| a.apply(Command::ToggleStatusLine, 0));
        shot("name", &|a| {
            a.open_field(
                Purpose::CreateAtLaunch,
                "New Document",
                "Document to be created:",
                "",
            )
        });
    }

    // --- focus routing ---

    fn key(a: &mut App, cmd: Command) {
        a.apply(cmd, T);
    }

    #[test]
    fn the_menu_bar_opens_and_closes() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        assert!(matches!(a.overlay(), Overlay::Menu(_)));
        key(&mut a, Command::Dismiss);
        assert!(
            matches!(a.overlay(), Overlay::None),
            "one Esc leaves the bar"
        );
    }

    #[test]
    fn typing_does_not_reach_the_document_while_the_menu_is_open() {
        let mut a = app_with("x");
        key(&mut a, Command::MenuBar);
        key(&mut a, Command::Insert("zzz".to_string()));
        assert_eq!(a.editor().to_string(), "x", "the menu took the letters");
    }

    #[test]
    fn choosing_a_menu_item_runs_its_command_and_closes_the_menu() {
        let mut a = App::new();
        key(&mut a, Command::Insert("hello".to_string()));
        key(&mut a, Command::MenuBar);
        key(&mut a, Command::Insert("e".to_string())); // Edit
        key(&mut a, Command::Insert("u".to_string())); // Undo
        assert_eq!(a.editor().to_string(), "", "Undo actually ran");
        assert!(matches!(a.overlay(), Overlay::None), "and the menu closed");
    }

    #[test]
    fn arrowing_to_an_item_and_pressing_enter_runs_it() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        key(&mut a, Command::Insert("v".to_string())); // View
        key(&mut a, Command::Newline); // first item: CRT Effects
        assert!(!a.effects(), "the toggle fired");
        assert!(matches!(a.overlay(), Overlay::None));
    }

    #[test]
    fn escape_backs_out_of_a_dropdown_before_leaving_the_bar() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        key(
            &mut a,
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
        );
        key(&mut a, Command::Dismiss);
        assert!(matches!(a.overlay(), Overlay::Menu(_)), "still on the bar");
        key(&mut a, Command::Dismiss);
        assert!(matches!(a.overlay(), Overlay::None));
    }

    #[test]
    fn a_modal_field_takes_typing_instead_of_the_document() {
        let mut a = app_with("doc");
        a.open_field(Purpose::SaveAs, "Save Document", "Filename:", "");
        key(&mut a, Command::Insert("notes.txt".to_string()));
        assert_eq!(a.editor().to_string(), "doc", "document untouched");
        let Overlay::Field(f) = a.overlay() else {
            panic!("field should still be open");
        };
        assert_eq!(f.value(), "notes.txt");
    }

    #[test]
    fn a_field_seeds_from_the_current_name() {
        let mut a = App::new();
        a.open_field(Purpose::SaveAs, "Save Document", "Filename:", "ch1.txt");
        let Overlay::Field(f) = a.overlay() else {
            panic!("field");
        };
        assert_eq!(f.value(), "ch1.txt");
        assert_eq!(f.cursor(), 7, "cursor ready at the end");
    }

    #[test]
    fn submitting_a_field_hands_the_value_over_once() {
        let mut a = App::new();
        a.open_field(Purpose::CreateAtLaunch, "New Document", "Name:", "");
        key(&mut a, Command::Insert("chapter-one.txt".to_string()));
        key(&mut a, Command::Newline);
        assert!(matches!(a.overlay(), Overlay::None), "the modal closed");
        assert_eq!(
            a.take_submitted(),
            Some((Purpose::CreateAtLaunch, "chapter-one.txt".to_string()))
        );
        assert_eq!(a.take_submitted(), None, "taken only once");
    }

    #[test]
    fn escaping_a_field_submits_nothing() {
        let mut a = App::new();
        a.open_field(Purpose::CreateAtLaunch, "New Document", "Name:", "");
        key(&mut a, Command::Insert("draft".to_string()));
        key(&mut a, Command::Dismiss);
        assert!(matches!(a.overlay(), Overlay::None));
        assert_eq!(a.take_submitted(), None, "Esc skips straight past");
    }

    #[test]
    fn an_empty_name_is_the_same_as_backing_out() {
        let mut a = App::new();
        a.open_field(Purpose::CreateAtLaunch, "New Document", "Name:", "");
        key(&mut a, Command::Insert("   ".to_string()));
        key(&mut a, Command::Newline);
        assert_eq!(a.take_submitted(), None);
    }

    #[test]
    fn editing_keys_work_inside_a_field() {
        let mut a = App::new();
        a.open_field(Purpose::SaveAs, "Save Document", "Filename:", "abc");
        key(&mut a, Command::Backspace);
        key(
            &mut a,
            Command::Move {
                motion: Motion::LineStart,
                extend: false,
            },
        );
        key(&mut a, Command::Insert("X".to_string()));
        let Overlay::Field(f) = a.overlay() else {
            panic!("field");
        };
        assert_eq!(f.value(), "Xab");
    }

    #[test]
    fn painting_a_menu_draws_the_bar_across_the_top() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        a.paint();
        let row: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, 0).glyph))
            .collect();
        assert!(row.contains("File"), "got: {row}");
        assert!(row.contains("Edit"), "got: {row}");
        assert!(row.contains("Help"), "got: {row}");
    }

    #[test]
    fn painting_a_dropdown_shows_labels_and_their_hotkeys() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        key(
            &mut a,
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
        );
        a.paint();
        let all: String = (0..a.screen().rows())
            .flat_map(|r| (0..80).map(move |c| (c, r)))
            .map(|(c, r)| cp437::decode(a.screen().cell(c, r).glyph))
            .collect();
        assert!(all.contains("Save As..."), "label missing");
        assert!(all.contains("F10"), "hotkey missing");
        assert!(all.contains("Exit"), "label missing");
    }

    #[test]
    fn painting_a_field_shows_its_title_label_and_value() {
        let mut a = App::new();
        a.open_field(
            Purpose::CreateAtLaunch,
            "New Document",
            "Document to be created:",
            "ch1.txt",
        );
        a.paint();
        let all: String = (0..a.screen().rows())
            .flat_map(|r| (0..80).map(move |c| (c, r)))
            .map(|(c, r)| cp437::decode(a.screen().cell(c, r).glyph))
            .collect();
        assert!(all.contains("New Document"), "title missing");
        assert!(all.contains("Document to be created:"), "label missing");
        assert!(all.contains("ch1.txt"), "value missing");
        assert!(all.contains("Esc"), "the way out is shown");
    }

    #[test]
    fn escape_opens_the_menu_when_the_document_has_focus() {
        let mut a = app_with("some words");
        key(&mut a, Command::Dismiss);
        assert!(
            matches!(a.overlay(), Overlay::Menu(_)),
            "Esc has nothing to dismiss, so it offers the menu"
        );
    }

    #[test]
    fn escape_still_closes_the_menu_rather_than_reopening_it() {
        let mut a = App::new();
        key(&mut a, Command::Dismiss); // opens
        key(&mut a, Command::Dismiss); // closes
        assert!(matches!(a.overlay(), Overlay::None), "not a loop");
    }

    #[test]
    fn escape_inside_a_field_cancels_instead_of_opening_the_menu() {
        let mut a = App::new();
        a.open_field(Purpose::SaveAs, "Save Document", "Filename:", "x");
        key(&mut a, Command::Dismiss);
        assert!(
            matches!(a.overlay(), Overlay::None),
            "cancelled, not a menu"
        );
        assert_eq!(a.take_submitted(), None);
    }

    #[test]
    fn escape_inside_a_confirmation_dismisses_it() {
        let mut a = App::new();
        key(&mut a, Command::Insert("x".to_string()));
        key(&mut a, Command::Quit);
        key(&mut a, Command::Dismiss);
        assert!(matches!(a.overlay(), Overlay::None));
        assert!(!a.should_quit);
    }

    #[test]
    fn escape_does_not_disturb_the_document() {
        let mut a = app_with("hello");
        key(
            &mut a,
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
        );
        key(&mut a, Command::Dismiss);
        key(&mut a, Command::Dismiss);
        assert_eq!(a.editor().to_string(), "hello");
        assert_eq!(a.editor().cursor(), 5, "and the caret stays put");
    }

    #[test]
    fn the_help_overlay_names_both_ways_into_the_menu() {
        let mut a = App::new();
        key(&mut a, Command::ToggleHelp);
        a.paint();
        let all: String = (0..a.screen().rows())
            .flat_map(|r| (0..80).map(move |c| (c, r)))
            .map(|(c, r)| cp437::decode(a.screen().cell(c, r).glyph))
            .collect();
        assert!(all.contains("Menu bar"), "help should name the menu key");
    }

    #[test]
    fn a_menu_item_the_shell_owns_is_handed_back_to_it() {
        let mut a = App::new();
        key(&mut a, Command::MenuBar);
        key(&mut a, Command::Insert("f".to_string())); // File
        key(&mut a, Command::Insert("s".to_string())); // Save
        assert!(matches!(a.overlay(), Overlay::None), "the menu closed");
        assert_eq!(a.take_deferred(), Some(Command::Save));
        assert_eq!(a.take_deferred(), None, "taken once");
    }

    #[test]
    fn a_hotkey_the_app_can_perform_is_not_deferred() {
        let mut a = App::new();
        let before = a.effects();
        key(&mut a, Command::MenuBar);
        key(&mut a, Command::Insert("v".to_string())); // View
        key(&mut a, Command::Insert("c".to_string())); // CRT Effects
        assert_ne!(a.effects(), before, "the toggle fired");
        assert_eq!(a.take_deferred(), None);
    }

    #[test]
    fn every_menu_item_either_acts_or_is_deferred() {
        for (mi, m) in menu::MENUS.iter().enumerate() {
            for it in m.items.iter() {
                let Some(cmd) = it.command.clone() else {
                    continue;
                };
                let mut a = app_with("some text");
                let before = (a.effects(), a.dense(), a.fullscreen(), a.status_line());
                key(&mut a, cmd.clone());
                let acted = a.take_deferred().is_some()
                    || !matches!(a.overlay(), Overlay::None)
                    || (a.effects(), a.dense(), a.fullscreen(), a.status_line()) != before
                    || a.should_quit
                    || matches!(cmd, Command::Undo | Command::Redo | Command::SelectAll)
                    || matches!(cmd, Command::ShowWordCount);
                assert!(
                    acted,
                    "{} > {} did nothing",
                    menu::MENUS[mi].title,
                    it.label
                );
            }
        }
    }

    #[test]
    fn the_print_screen_offers_full_document_page_and_disk() {
        let mut a = app_with("hello");
        a.open_print(Some("LPT1".to_string()));
        a.paint();
        let all: String = (0..a.screen().rows())
            .flat_map(|r| (0..80).map(move |c| (c, r)))
            .map(|(c, r)| cp437::decode(a.screen().cell(c, r).glyph))
            .collect();
        assert!(all.contains("1 - Full Document"), "got: {all}");
        assert!(all.contains("2 - Page"));
        assert!(all.contains("3 - Document to Disk"));
        assert!(all.contains("LPT1"), "shows the printer");
        assert!(all.contains("Selection: 0"));
        let bottom = a.screen().rows() - 1;
        assert_eq!(a.screen().cursor(), Some((11, bottom)), "cursor on the 0");
    }

    #[test]
    fn choosing_one_asks_for_the_full_document() {
        let mut a = app_with("hello");
        a.open_print(None);
        key(&mut a, Command::Insert("1".to_string()));
        assert!(matches!(a.overlay(), Overlay::None));
        assert_eq!(a.take_print_request(), Some(Request::Printer(Scope::Full)));
    }

    #[test]
    fn choosing_two_asks_for_the_cursor_page() {
        let text = (0..60)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = app_with(&text);
        a.apply(
            Command::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
            T,
        );
        a.open_print(None);
        key(&mut a, Command::Insert("2".to_string()));
        assert_eq!(
            a.take_print_request(),
            Some(Request::Printer(Scope::Page(1)))
        );
    }

    #[test]
    fn choosing_three_prints_to_disk() {
        let mut a = app_with("hello");
        a.open_print(None);
        key(&mut a, Command::Insert("3".to_string()));
        assert_eq!(a.take_print_request(), Some(Request::ToFile(Scope::Full)));
    }

    #[test]
    fn zero_or_escape_leaves_the_print_screen_with_no_job() {
        for cmd in [Command::Insert("0".to_string()), Command::Dismiss] {
            let mut a = app_with("hello");
            a.open_print(None);
            key(&mut a, cmd);
            assert!(matches!(a.overlay(), Overlay::None));
            assert_eq!(a.take_print_request(), None);
        }
    }

    #[test]
    fn a_stray_key_on_the_print_screen_is_ignored() {
        let mut a = app_with("hello");
        a.open_print(None);
        key(&mut a, Command::Insert("x".to_string()));
        assert!(matches!(a.overlay(), Overlay::Print { .. }), "still open");
        assert_eq!(a.editor().to_string(), "hello", "nothing typed through");
    }

    #[test]
    fn an_empty_document_has_no_print_job() {
        let a = app_with("   \n\n");
        assert_eq!(a.print_job(Scope::Full), None);
        let a = app_with("hello");
        assert!(a.print_job(Scope::Full).unwrap().contains("hello"));
    }

    #[test]
    fn a_notice_borrows_the_status_line_and_then_expires() {
        let mut a = app_with("hello");
        a.paint_at(1_000);
        a.notify("Sent 1 page to LPT1");
        a.paint_at(1_500);
        let row = a.screen().rows() - 1;
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(line.contains("Sent 1 page to LPT1"), "got: {line}");
        a.paint_at(5_000);
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(line.contains("Pg 1"), "back to the readout: {line}");
    }

    #[test]
    fn the_help_overlay_names_print() {
        assert!(HELP_TEXT.contains("Shft-F7  Print"));
        assert!(HELP_TEXT.contains("Cmd-P  Print"));
    }
}
