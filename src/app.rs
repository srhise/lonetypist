//! Application state: the document, where it is on screen, and the
//! interpretation of every command.

use std::path::PathBuf;

use crate::cp437;
use crate::editor::Editor;
use crate::fileio::TAB_STOP;
use crate::fountain::{self, Caret, Element};
use crate::input::{Form, Input, Purpose};
use crate::keymap::{Command, Motion};
use crate::menu::{self, MenuState};
use crate::overlay::{self, Overlay, Prompt};
use crate::print::{self, Request, Scope};
use crate::script::{self, Layout, Row};
use crate::sound::Sound;
use crate::status;
use crate::typewriter::{self, Ribbon, Sheet};
use crate::vga::{Mode, Screen};
use crate::wrap::{self, VisualLine};

/// Body text and status line colours (Global Constraints).
const FG: u8 = 7;
/// The screen's background. The shell also paints the letterbox slack
/// around the picture in it, so the blue runs to the window's edge.
pub const BG: u8 = 1;
const STATUS_FG: u8 = 15;
/// Scene headings stand out, as WordPerfect showed codes in their own
/// colour.
const HEADING_FG: u8 = 15;
/// What is on the page but not in the text -- the page rule, (MORE),
/// (CONT'D) -- and what completion offers but you have not typed.
const DIM_FG: u8 = 8;
/// Notes, sections and the boneyard: in the file, never on paper.
const NOTE_FG: u8 = 3;

/// A new screenplay opens on the first words of every screenplay.
const SCREENPLAY_SEED: &str = "FADE IN:\n\n";

/// The title page fields, in the order the form shows them.
const TITLE_FIELDS: &[(&str, &str)] = &[
    ("title", "Title"),
    ("credit", "Credit"),
    ("author", "Author"),
    ("source", "Source"),
    ("draft date", "Draft date"),
    ("contact", "Contact"),
];
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
F8  Screenplay mode   Cmd-Shift-N  New screenplay
Shft-F1  This help    F9  Typewriter sounds

In a screenplay
Enter  Next element   Tab  Cue, transition, heading
Tab in speech  ( )    Cmd-G  Go to scene

Esc  Close this";

pub struct App {
    editor: Editor,
    screen: Screen,
    path: Option<PathBuf>,
    crlf: bool,
    /// First visual line shown in the viewport.
    viewport_top: usize,
    /// Cached layout of the whole document, rebuilt when the text
    /// changes -- and, in a screenplay, when the caret changes line.
    layout: Layout,
    /// The caret context the layout was built for.
    layout_caret: Caret,
    /// Writing a screenplay rather than prose.
    screenplay: bool,
    /// An element chosen with Tab for an empty line, with the offset the
    /// line starts at. It lasts while the caret stays on that line.
    pending: Option<(usize, Element)>,
    /// How the typewriter strikes when a screenplay prints.
    ribbon: Ribbon,
    /// Typewriter sounds, and the ones waiting for the shell to play.
    sounds_on: bool,
    sounds: Vec<Sound>,
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
        let layout = Layout::prose(editor.text());
        Self {
            editor,
            screen: Screen::new(Mode::Text80x25),
            path: None,
            crlf: false,
            viewport_top: 0,
            layout,
            layout_caret: Caret::default(),
            screenplay: false,
            pending: None,
            ribbon: Ribbon::default(),
            sounds_on: false,
            sounds: Vec::new(),
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
        self.pending = None;
        self.reflow();
    }

    /// Start a new screenplay: the seed text, the caret under it.
    pub fn start_screenplay(&mut self) {
        self.screenplay = true;
        self.load_text(SCREENPLAY_SEED);
        self.editor.set_cursor(self.editor.len(), false);
        self.reflow();
    }

    pub fn screenplay(&self) -> bool {
        self.screenplay
    }

    pub fn set_screenplay(&mut self, on: bool) {
        self.screenplay = on;
        self.pending = None;
        self.goal_col = None;
        self.reflow();
        self.scroll_to_cursor();
    }

    pub fn ribbon(&self) -> Ribbon {
        self.ribbon
    }

    pub fn set_ribbon(&mut self, ribbon: Ribbon) {
        self.ribbon = ribbon;
    }

    pub fn sounds_on(&self) -> bool {
        self.sounds_on
    }

    pub fn set_sounds(&mut self, on: bool) {
        self.sounds_on = on;
        if !on {
            self.sounds.clear();
        }
    }

    /// The sounds the last commands made, for the shell to play.
    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    fn sound(&mut self, s: Sound) {
        if self.sounds_on {
            self.sounds.push(s);
        }
    }

    /// The strike a typed character makes, and the bell when it brings
    /// the carriage seven characters short of the margin.
    fn strike(&mut self, typed: &str) {
        if !self.sounds_on || typed.chars().count() != 1 {
            return;
        }
        self.sound(if typed == " " {
            Sound::Space
        } else {
            Sound::Key
        });
        let (line, col) = self.cursor_position();
        let width = if self.screenplay {
            let kind = self.layout.kinds.get(line).copied();
            script::geometry(kind.unwrap_or(Element::Action)).1
        } else {
            wrap::TEXT_COLS
        };
        if col + 7 == width {
            self.sound(Sound::Bell);
        }
    }

    fn reflow(&mut self) {
        let caret = if self.screenplay {
            self.caret()
        } else {
            Caret::default()
        };
        self.layout = if self.screenplay {
            Layout::screenplay(self.editor.text(), caret)
        } else {
            Layout::prose(self.editor.text())
        };
        self.layout_caret = caret;
    }

    /// The grid column of the text margin.
    fn left(&self) -> usize {
        if self.screenplay {
            script::TEXT_LEFT
        } else {
            wrap::TEXT_LEFT
        }
    }

    fn lines(&self) -> &[VisualLine] {
        &self.layout.lines
    }

    fn indent(&self, line: usize) -> usize {
        let lines = self.lines();
        lines
            .get(line.min(lines.len().saturating_sub(1)))
            .map_or(0, |l| l.indent)
    }

    /// The source line the caret is on: its index, start and end.
    fn caret_source(&self) -> (usize, usize, usize) {
        let text = self.editor.text();
        let cursor = self.editor.cursor().min(text.len());
        let start = text[..cursor]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1);
        let end = text[cursor..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(text.len(), |i| cursor + i);
        let index = text[..start].iter().filter(|&&c| c == '\n').count();
        (index, start, end)
    }

    fn caret(&self) -> Caret {
        let (line, start, _) = self.caret_source();
        Caret {
            line: Some(line),
            pending: self
                .pending
                .filter(|(at, _)| *at == start)
                .map(|(_, el)| el),
        }
    }

    /// The element under the caret.
    fn caret_element(&self) -> Element {
        let (line, _, _) = self.caret_source();
        self.layout
            .source_kinds
            .get(line)
            .copied()
            .unwrap_or(Element::Action)
    }

    /// In a screenplay the caret's line shapes the layout, so moving it
    /// to another line lays the script out again. A Tab choice is left
    /// behind with the line it was made on.
    fn follow_caret(&mut self) {
        if !self.screenplay {
            return;
        }
        let caret = self.caret();
        if caret.pending.is_none() {
            self.pending = None;
        }
        if caret != self.layout_caret {
            self.reflow();
        }
    }

    fn cursor_position(&self) -> (usize, usize) {
        wrap::position_of(self.lines(), self.editor.cursor())
    }

    /// The screen row, counted over the whole document, the caret is on.
    fn cursor_row(&self) -> usize {
        let (line, _) = self.cursor_position();
        self.layout.row_of_line.get(line).copied().unwrap_or(0)
    }

    /// Scroll the minimum distance that puts the cursor back on screen.
    fn scroll_to_cursor(&mut self) {
        let row = self.cursor_row();
        let rows = self.text_rows();
        if row < self.viewport_top {
            self.viewport_top = row;
        } else if row >= self.viewport_top + rows {
            self.viewport_top = row + 1 - rows;
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
        // through a short line does not lose your place. The column is
        // the one on screen, so in a screenplay a move from dialogue to
        // action keeps its place on the page, not in the line.
        let vertical = matches!(
            motion,
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown
        );
        let screen_col = self.indent(line) + col;
        let goal = if vertical {
            let g = self.goal_col.unwrap_or(screen_col);
            self.goal_col = Some(g);
            g
        } else {
            self.goal_col = None;
            screen_col
        };
        let at = |l: usize| wrap::offset_at(self.lines(), l, goal.saturating_sub(self.indent(l)));

        let target = match motion {
            Motion::Left => cursor.saturating_sub(1),
            Motion::Right => (cursor + 1).min(self.editor.len()),
            Motion::WordLeft => self.editor.word_left(),
            Motion::WordRight => self.editor.word_right(),
            Motion::LineStart => wrap::offset_at(self.lines(), line, 0),
            Motion::LineEnd => wrap::offset_at(self.lines(), line, usize::MAX),
            Motion::DocStart => 0,
            Motion::DocEnd => self.editor.len(),
            Motion::Up => at(line.saturating_sub(1)),
            Motion::Down => at(line + 1),
            Motion::PageUp => at(line.saturating_sub(rows)),
            Motion::PageDown => at(line + rows),
        };

        self.editor.set_cursor(target, extend);
        self.follow_caret();
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
        if self.screenplay {
            self.layout.page_count()
        } else {
            print::page_count(self.lines())
        }
    }

    /// The zero-based page the caret is on.
    fn caret_page(&self) -> usize {
        let (line, _) = self.cursor_position();
        if self.screenplay {
            self.layout.page_of_line.get(line).copied().unwrap_or(0)
        } else {
            print::page_of(line)
        }
    }

    /// The print job text for `scope`, or `None` if there is nothing to
    /// print or no such page.
    pub fn print_job(&self, scope: Scope) -> Option<String> {
        if self.editor.to_string().trim().is_empty() {
            return None;
        }
        print::job(self.editor.text(), self.lines(), scope)
    }

    /// A screenplay's print job: a typewritten PDF. The whole script
    /// opens with its title page; a single page goes alone, with its
    /// number. `None` if there is nothing to print or no such page.
    pub fn script_pdf(&self, scope: Scope) -> Option<Vec<u8>> {
        let source = self.editor.to_string();
        if source.trim().is_empty() {
            return None;
        }
        // Laid out as the file stands, without the caret's say.
        let layout = Layout::screenplay(self.editor.text(), Caret::default());
        let pages = layout.print_pages(self.editor.text());
        let sheets: Vec<Sheet> = pages
            .iter()
            .enumerate()
            .map(|(i, lines)| Sheet {
                number: i + 1,
                lines,
            })
            .collect();
        match scope {
            Scope::Full => Some(typewriter::render(
                &script::title_page(&source),
                &sheets,
                self.ribbon,
            )),
            Scope::Page(n) => sheets
                .get(n)
                .map(|s| typewriter::render(&[], std::slice::from_ref(s), self.ribbon)),
        }
    }

    /// Keys reaching the print screen: a digit picks, 0 or Esc leaves.
    fn print_key(&mut self, cmd: Command) {
        let choice = match cmd {
            Command::Insert(text) => text.chars().next(),
            Command::Dismiss | Command::Newline => Some('0'),
            _ => None,
        };
        let page = Scope::Page(self.caret_page());
        match choice {
            Some('r' | 'R') if self.screenplay => {
                self.ribbon = self.ribbon.next();
                return;
            }
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

    /// The menu bar with File pulled down: how the app greets you, so
    /// New, Open and Draft Screenplay are the first thing on screen.
    /// Esc puts it away and the blank page is underneath.
    pub fn open_file_menu(&mut self) {
        self.overlay = Overlay::Menu(MenuState::greeting());
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
        // The launch menu stands aside for anyone who just starts
        // writing: a keystroke closes it and lands on the page, and one
        // Esc is enough.
        if state.is_greeting() {
            match cmd {
                Command::Move { .. } => state.engage(),
                Command::Newline => {}
                Command::Dismiss => {
                    self.overlay = Overlay::None;
                    return None;
                }
                other => {
                    self.overlay = Overlay::None;
                    return Some(other);
                }
            }
        }
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

    /// Typing in a screenplay. Headings, cues and transitions are in
    /// capitals, so letters typed into them come out that way; a paste
    /// keeps its own case.
    fn script_insert(&mut self, s: &str, now_ms: u64) {
        let typed = !s.contains('\n');
        let s = if typed && self.caret_element().capitalised() {
            upper(s)
        } else {
            s.to_string()
        };
        self.editor.insert(&s, now_ms);
        self.after_edit();
        // `int.` becomes a heading at the full stop: raise what came
        // before it too.
        if typed && self.caret_element() == Element::SceneHeading {
            self.raise_line(now_ms);
        }
    }

    /// Put the caret's whole line in capitals, leaving the caret where
    /// it was.
    fn raise_line(&mut self, now_ms: u64) {
        let (_, start, end) = self.caret_source();
        let line: String = self.editor.text()[start..end].iter().collect();
        let raised = upper(&line);
        if raised == line {
            return;
        }
        let cursor = self.editor.cursor();
        self.replace_range(start, end, &raised, now_ms);
        self.editor.set_cursor(cursor, false);
        self.after_edit();
    }

    /// Replace `start..end` with `s` as one edit.
    fn replace_range(&mut self, start: usize, end: usize, s: &str, now_ms: u64) {
        self.editor.set_cursor(start, false);
        self.editor.set_cursor(end, true);
        if s.is_empty() {
            self.editor.backspace(now_ms);
        } else {
            self.editor.insert(s, now_ms);
        }
    }

    /// Whether an action line would read as a character cue if dialogue
    /// followed: capitals after a blank line, not a heading, transition
    /// or `FADE IN:`. A name does not end a sentence, so `BOOM!` and
    /// `A.` stay action.
    fn would_be_cue(&self, start: usize, line: &str) -> bool {
        let text = self.editor.text();
        let after_blank = start == 0 || (start >= 2 && text[start - 2] == '\n') || start == 1;
        let t = line.trim();
        after_blank
            && !t.is_empty()
            && fountain::is_cue(t)
            && !t.ends_with(['.', '!', '?', ':', ',', ';'])
            && !fountain::is_scene_heading(t)
    }

    /// Enter in a screenplay ends the element and starts the one that
    /// follows it: a cue gets its dialogue on the next line, a speech or
    /// a paragraph gets a blank line and then action.
    fn script_newline(&mut self, now_ms: u64) {
        if self.editor.selection().is_some() {
            self.editor.insert("\n", now_ms);
            self.after_edit();
            return;
        }
        let el = self.caret_element();
        let (_, start, end) = self.caret_source();
        let cursor = self.editor.cursor();
        let tail: String = self.editor.text()[cursor..end].iter().collect();
        if el == Element::Parenthetical && tail == ")" {
            // Enter inside the parentheses steps out of them.
            self.editor.set_cursor(end, false);
        } else if cursor != end {
            // In the middle of a line Enter splits it, as in prose.
            self.pending = None;
            self.editor.insert("\n", now_ms);
            self.after_edit();
            return;
        }
        let line: String = self.editor.text()[start..end].iter().collect();
        let cue_like =
            el == Element::Character || (el == Element::Action && self.would_be_cue(start, &line));
        if cue_like {
            if let Some(rest) = self.completion() {
                self.editor.insert(&rest, now_ms);
            }
        }
        let pending = self.caret().pending;
        self.pending = None;
        let line: String = {
            let (_, start, end) = self.caret_source();
            self.editor.text()[start..end].iter().collect()
        };
        if line.trim().is_empty() {
            self.editor.insert("\n", now_ms);
            self.after_edit();
            return;
        }
        // A line made a heading or transition with Tab keeps that
        // element once the caret leaves it, by Fountain's forcing mark.
        let t = line.trim();
        let force = match pending {
            Some(Element::SceneHeading)
                if !fountain::is_scene_heading(t) && !t.starts_with('.') =>
            {
                "."
            }
            Some(Element::Transition) if !t.ends_with("TO:") && !t.starts_with('>') => "> ",
            Some(Element::Character) if !fountain::is_cue(t) && !t.starts_with('@') => "@",
            _ => "",
        };
        if !force.is_empty() {
            let at = self.editor.cursor();
            self.editor.set_cursor(start, false);
            self.editor.insert(force, now_ms);
            self.editor.set_cursor(at + force.len(), false);
        }
        let breaks = match el {
            Element::SceneHeading | Element::Transition => {
                self.raise_line(now_ms);
                "\n\n"
            }
            Element::Character => {
                self.raise_line(now_ms);
                "\n"
            }
            Element::Action if cue_like => {
                self.raise_line(now_ms);
                "\n"
            }
            Element::Parenthetical | Element::TitlePage => "\n",
            _ => "\n\n",
        };
        self.editor.set_cursor(self.caret_source().2, false);
        self.editor.insert(breaks, now_ms);
        self.after_edit();
    }

    /// Tab in a screenplay. It takes what completion offers; on an empty
    /// line it picks the element the line will be; in a speech it opens
    /// a parenthetical, or steps out of one.
    fn script_tab(&mut self, now_ms: u64) {
        if self.completion().is_some() {
            self.accept_completion(now_ms);
            return;
        }
        let el = self.caret_element();
        let (_, start, end) = self.caret_source();
        let cursor = self.editor.cursor();
        let text = self.editor.text();
        let empty = text[start..end].iter().all(|c| c.is_whitespace());
        let tail: String = text[cursor..end].iter().collect();

        if empty && el != Element::Dialogue {
            let next = match self.caret().pending {
                None => Some(Element::Character),
                Some(Element::Character) => Some(Element::Transition),
                Some(Element::Transition) => Some(Element::SceneHeading),
                Some(_) => None,
            };
            self.pending = next.map(|el| (start, el));
            self.reflow();
            if let Some(el) = next {
                self.notify(el.name());
            }
            return;
        }
        match el {
            Element::Dialogue | Element::Character if cursor == end => {
                let open = if empty { "()" } else { "\n()" };
                self.editor.insert(open, now_ms);
                self.editor.set_cursor(self.editor.cursor() - 1, false);
                self.after_edit();
            }
            Element::Parenthetical if tail == ")" => {
                self.editor.set_cursor(end, false);
                self.editor.insert("\n", now_ms);
                self.after_edit();
            }
            _ => {}
        }
    }

    /// What completion offers at the caret: the rest of a character's
    /// name, a location after INT. or EXT., or a time after the dash.
    fn completion(&self) -> Option<String> {
        if !self.screenplay || self.editor.selection().is_some() {
            return None;
        }
        let (_, start, end) = self.caret_source();
        if self.editor.cursor() != end {
            return None;
        }
        let line: String = self.editor.text()[start..end].iter().collect();
        let raised = upper(&line);
        let offer = |typed: &str, from: &[String]| -> Option<String> {
            let typed = typed.trim_start();
            if typed.is_empty() {
                return None;
            }
            from.iter()
                .find(|c| c.starts_with(typed) && c.len() > typed.len())
                .map(|c| c[typed.len()..].to_string())
        };
        match self.caret_element() {
            Element::Character => {
                let typed = raised.trim_start_matches('@');
                (!typed.contains('(')).then(|| offer(typed, &self.layout.characters))?
            }
            Element::Action if self.would_be_cue(start, &line) => {
                offer(&raised, &self.layout.characters)
            }
            Element::SceneHeading => {
                if let Some(dash) = raised.rfind(" - ") {
                    return offer(&raised[dash + 3..], &self.layout.times);
                }
                let body = match fountain::scene_prefix_len(raised.trim_start()) {
                    Some(n) => &raised.trim_start()[n..],
                    None => raised.trim_start().strip_prefix('.')?,
                };
                offer(body, &self.layout.locations)
            }
            _ => None,
        }
    }

    fn accept_completion(&mut self, now_ms: u64) {
        if let Some(rest) = self.completion() {
            self.editor.insert(&rest, now_ms);
            self.after_edit();
        }
    }

    /// Keys reaching the scene list.
    fn scenes_key(&mut self, cmd: Command) {
        let Overlay::Scenes { selected } = &mut self.overlay else {
            return;
        };
        let count = self.layout.scenes.len();
        let last = count.saturating_sub(1);
        match cmd {
            Command::Move { motion, .. } => {
                *selected = match motion {
                    Motion::Up | Motion::Left => selected.saturating_sub(1),
                    Motion::Down | Motion::Right => (*selected + 1).min(last),
                    Motion::PageUp => selected.saturating_sub(10),
                    Motion::PageDown => (*selected + 10).min(last),
                    Motion::DocStart | Motion::LineStart => 0,
                    Motion::DocEnd | Motion::LineEnd => last,
                    _ => *selected,
                }
            }
            Command::Newline => {
                let at = *selected;
                self.overlay = Overlay::None;
                if let Some(scene) = self.layout.scenes.get(at) {
                    self.editor.set_cursor(scene.offset, false);
                    self.goal_col = None;
                    self.follow_caret();
                    // The heading opens the screen, a row of context
                    // above it.
                    self.viewport_top = self.cursor_row().saturating_sub(1);
                    self.scroll_to_cursor();
                }
            }
            Command::Dismiss => self.overlay = Overlay::None,
            _ => {}
        }
    }

    fn open_title_page(&mut self) {
        let current = script::title_page(&self.editor.to_string());
        let fields: Vec<(&str, &str, String)> = TITLE_FIELDS
            .iter()
            .map(|&(key, label)| {
                let value = current
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.replace('\n', " | "))
                    .unwrap_or_else(|| {
                        if key == "credit" {
                            "Written by".to_string()
                        } else {
                            String::new()
                        }
                    });
                (key, label, value)
            })
            .collect();
        self.overlay = Overlay::TitlePage(Form::new(&fields));
    }

    /// Keys reaching the title page form.
    fn title_key(&mut self, cmd: Command, now_ms: u64) {
        let Overlay::TitlePage(form) = &mut self.overlay else {
            return;
        };
        match cmd {
            Command::Insert(text) => form.focused().insert(&text),
            Command::Backspace => form.focused().backspace(),
            Command::DeleteForward => form.focused().delete(),
            Command::Tab
            | Command::Move {
                motion: Motion::Down,
                ..
            } => form.next(),
            Command::Move {
                motion: Motion::Up, ..
            } => form.prev(),
            Command::Move {
                motion: Motion::Left,
                ..
            } => form.focused().left(),
            Command::Move {
                motion: Motion::Right,
                ..
            } => form.focused().right(),
            Command::Move {
                motion: Motion::LineStart,
                ..
            } => form.focused().home(),
            Command::Move {
                motion: Motion::LineEnd,
                ..
            } => form.focused().end(),
            Command::Newline => {
                let form = form.clone();
                self.overlay = Overlay::None;
                self.write_title_page(&form, now_ms);
            }
            Command::Dismiss => self.overlay = Overlay::None,
            _ => {}
        }
    }

    /// Replace the title page block at the top of the file with the
    /// form's fields. A value split with `|` runs onto indented lines.
    fn write_title_page(&mut self, form: &Form, now_ms: u64) {
        let mut block = String::new();
        for (key, input) in &form.fields {
            let value = input.value();
            let parts: Vec<&str> = value
                .split('|')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            let label = TITLE_FIELDS
                .iter()
                .find(|(k, _)| k == key)
                .map_or(key.as_str(), |(_, l)| *l);
            match parts.as_slice() {
                [] => {}
                [one] => block.push_str(&format!("{label}: {one}\n")),
                many => {
                    block.push_str(&format!("{label}:\n"));
                    for part in many {
                        block.push_str(&format!("    {part}\n"));
                    }
                }
            }
        }
        if !block.is_empty() {
            block.push('\n');
        }

        // The old block: its lines, and the blank lines after it.
        let text = self.editor.text();
        let mut end = 0;
        if self.layout.source_kinds.first() == Some(&Element::TitlePage) {
            let mut kinds = self.layout.source_kinds.iter();
            let mut at = 0;
            while kinds.next() == Some(&Element::TitlePage) {
                at = text[at..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(text.len(), |i| at + i + 1);
            }
            while at < text.len() && text[at] == '\n' {
                at += 1;
            }
            end = at;
        }
        let old: String = text[..end].iter().collect();
        if old == block {
            return;
        }
        let cursor = self.editor.cursor();
        let moved = if cursor >= end {
            cursor - end + block.chars().count()
        } else {
            block.chars().count()
        };
        self.replace_range(0, end, &block, now_ms);
        self.editor.set_cursor(moved, false);
        self.after_edit();
        self.notify("Title page saved");
    }

    fn paint_scenes(&mut self, selected: usize) {
        let fg = 15;
        let bg = BG;
        if self.layout.scenes.is_empty() {
            overlay::draw_centered(
                &mut self.screen,
                "Go to Scene",
                "No scene headings yet.\n\nStart one with INT. or EXT.,\nor press Tab three times\non an empty line.",
                fg,
                bg,
            );
            return;
        }
        let width = 72;
        let x = (self.screen.cols() - width) / 2;
        let visible = self
            .layout
            .scenes
            .len()
            .min(self.screen.rows().saturating_sub(8))
            .max(1);
        let height = visible + 4;
        let y = (self.screen.rows().saturating_sub(height)) / 2;
        overlay::draw_box(&mut self.screen, x, y, width, height, fg, bg);
        let title = " Go to Scene ";
        self.screen
            .put_str(x + (width - title.len()) / 2, y, title, fg, bg);

        let top = if selected < visible {
            0
        } else {
            selected + 1 - visible
        };
        let inner = width - 4;
        for (i, scene) in self
            .layout
            .scenes
            .iter()
            .enumerate()
            .skip(top)
            .take(visible)
        {
            let page = format!("{}.", scene.page + 1);
            let number = format!("{:>3}  ", i + 1);
            let room = inner - number.len() - page.len() - 1;
            let heading: String = scene.heading.chars().take(room).collect();
            let row = format!("{number}{heading:<room$} {page}");
            let (rfg, rbg) = if i == selected { (0, 7) } else { (fg, bg) };
            let ry = y + 1 + (i - top);
            for c in 1..width - 1 {
                self.screen.set(x + c, ry, 0x20, rfg, rbg);
            }
            self.screen.put_str(x + 2, ry, &row, rfg, rbg);
        }
        let help = "Enter  Go     Esc  Cancel";
        self.screen
            .put_str(x + (width - help.len()) / 2, y + height - 2, help, fg, bg);
    }

    fn paint_title_page(&mut self, form: &Form, blink_on: bool) {
        let fg = 15;
        let bg = BG;
        let inner = 44usize;
        let label_w = 12;
        let width = label_w + inner + 8;
        let height = form.fields.len() * 2 + 6;
        let x = (self.screen.cols() - width) / 2;
        let y = (self.screen.rows().saturating_sub(height)) / 2;
        overlay::draw_box(&mut self.screen, x, y, width, height, fg, bg);
        let title = " Title Page ";
        self.screen
            .put_str(x + (width - title.len()) / 2, y, title, fg, bg);

        for (i, (_, field)) in form.fields.iter().enumerate() {
            let fy = y + 2 + i * 2;
            let label = format!("{:>label_w$}", field.label());
            self.screen.put_str(x + 3, fy, &label, fg, bg);
            let fx = x + 3 + label_w + 2;
            for c in 0..inner {
                self.screen.set(fx + c, fy, 0x20, 0, 7);
            }
            let value: Vec<char> = field.value().chars().collect();
            let offset = field.cursor().saturating_sub(inner.saturating_sub(1));
            let shown: String = value.iter().skip(offset).take(inner).collect();
            self.screen.put_str(fx, fy, &shown, 0, 7);
            if i == form.focus && blink_on {
                let cx = fx + (field.cursor() - offset).min(inner - 1);
                self.screen.set_cursor(Some((cx, fy)));
            }
        }
        let help = "Tab  Next   Enter  Save   Esc  Cancel   |  New line";
        self.screen.put_str(
            x + (width.saturating_sub(help.len())) / 2,
            y + height - 2,
            help,
            fg,
            bg,
        );
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
            Overlay::Scenes { .. } => {
                self.scenes_key(cmd);
                return;
            }
            Overlay::TitlePage(_) => {
                self.title_key(cmd, now_ms);
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
                if self.screenplay {
                    self.script_insert(&filtered, now_ms);
                } else {
                    self.editor.insert(&filtered, now_ms);
                    self.after_edit();
                }
                self.strike(&filtered);
            }
            Command::Newline if self.screenplay => {
                self.sound(Sound::Return);
                self.script_newline(now_ms);
            }
            Command::Newline => {
                self.sound(Sound::Return);
                self.editor.insert("\n", now_ms);
                self.after_edit();
            }
            Command::Tab if self.screenplay => {
                self.sound(Sound::Key);
                self.script_tab(now_ms);
            }
            Command::Tab => {
                self.sound(Sound::Key);
                let (_, col) = self.cursor_position();
                let spaces = TAB_STOP - (col % TAB_STOP);
                self.editor.insert(&" ".repeat(spaces), now_ms);
                self.after_edit();
            }
            Command::Backspace => {
                self.sound(Sound::Key);
                self.editor.backspace(now_ms);
                self.after_edit();
            }
            Command::DeleteForward => {
                self.sound(Sound::Key);
                self.editor.delete_forward(now_ms);
                self.after_edit();
            }
            Command::Move {
                motion: Motion::Right,
                extend: false,
            } if self.completion().is_some() => self.accept_completion(now_ms),
            Command::Move { motion, extend } => self.move_cursor(motion, extend),
            Command::SelectAll => {
                self.editor.set_cursor(0, false);
                self.editor.set_cursor(self.editor.len(), true);
                self.follow_caret();
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
            Command::ToggleSounds => {
                let on = !self.sounds_on;
                self.set_sounds(on);
                self.notify(if on {
                    "Typewriter sounds on"
                } else {
                    "Typewriter sounds off"
                });
            }
            Command::ToggleScreenplay => {
                let on = !self.screenplay;
                self.set_screenplay(on);
                self.notify(if on { "Screenplay" } else { "Prose" });
            }
            Command::SceneList if self.screenplay => {
                let here = self.layout.scene_at(self.editor.cursor());
                self.overlay = Overlay::Scenes {
                    selected: here.saturating_sub(1),
                };
            }
            Command::TitlePage if self.screenplay => self.open_title_page(),
            Command::SceneList | Command::TitlePage => {
                self.notify("Screenplays only: F8 switches");
            }
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
            | Command::NewScreenplay
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
        self.lines()
    }

    /// The character offset under a grid cell.
    pub fn offset_at_cell(&self, col: usize, row: usize) -> usize {
        let r = self.viewport_top + row.saturating_sub(wrap::TEXT_TOP);
        let rows = &self.layout.rows;
        // Clicking in the empty space below the text lands at the end of
        // the document, not at the start of the last line.
        if r >= rows.len() {
            return self.editor.len();
        }
        // A row that is not text -- the page rule, (MORE) -- lands on the
        // next line of text, or the last if there is none after it.
        let line = rows[r..]
            .iter()
            .chain(rows[..r].iter().rev())
            .find_map(|row| match row {
                Row::Line(li) => Some(*li),
                _ => None,
            })
            .unwrap_or(0);
        let col = col.saturating_sub(self.left() + self.indent(line));
        wrap::offset_at(self.lines(), line, col)
    }

    pub fn click(&mut self, col: usize, row: usize, extend: bool) {
        self.last_input_ms = self.now_ms;
        let offset = self.offset_at_cell(col, row);
        self.editor.set_cursor(offset, extend);
        self.follow_caret();
        self.goal_col = None;
        self.scroll_to_cursor();
    }

    /// Scroll without moving the caret, as a scroll wheel should.
    pub fn scroll(&mut self, lines: i32) {
        let max_top = self.layout.rows.len().saturating_sub(self.text_rows());
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
        let left = self.left();
        let cue = left + script::CUE_COL - script::ACTION_COL;
        let text = self.editor.text();

        for row in 0..rows {
            let Some(r) = self.layout.rows.get(self.viewport_top + row) else {
                break;
            };
            let y = wrap::TEXT_TOP + row;
            match r {
                Row::Line(li) => {
                    let line = self.layout.lines[*li];
                    let s: String = text[line.start..line.end].iter().collect();
                    let fg = match self.layout.kinds[*li] {
                        _ if !self.screenplay => FG,
                        Element::SceneHeading => HEADING_FG,
                        el if !el.prints() || el == Element::TitlePage => NOTE_FG,
                        _ => FG,
                    };
                    self.screen.put_str(left + line.indent, y, &s, fg, BG);
                }
                Row::Break(page) => {
                    // The rule a page break was drawn with, carrying the
                    // number the new page will be typed with.
                    for c in left..left + script::BLOCK_WIDTH {
                        self.screen.set(c, y, 0xC4, DIM_FG, BG);
                    }
                    let label = format!(" {page}. ");
                    let at = left + script::BLOCK_WIDTH - label.len() - 1;
                    self.screen.put_str(at, y, &label, DIM_FG, BG);
                }
                Row::More => {
                    self.screen.put_str(cue, y, "(MORE)", DIM_FG, BG);
                }
                Row::Contd(name) => {
                    self.screen.put_str(cue, y, name, DIM_FG, BG);
                }
            }
        }

        self.paint_completion();
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
        matches!(
            self.overlay,
            Overlay::Field(_) | Overlay::Print { .. } | Overlay::TitlePage(_)
        )
    }

    /// WordPerfect's Shift-F7 screen: the document goes away, the
    /// choices sit at the top left, and "Selection: 0" waits at the
    /// bottom with the cursor on the 0.
    fn paint_print(&mut self, printer: Option<&str>) {
        self.screen.clear(FG, BG);
        let rows = self.screen.rows();
        let here = self.caret_page() + 1;
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
        if self.screenplay {
            let ribbon = format!("R - Ribbon            {}", self.ribbon.name());
            self.screen.put_str(5, 10, &ribbon, FG, BG);
            self.screen
                .put_str(5, 12, "Typed on Courier 10 Pitch, as a PDF.", DIM_FG, BG);
        }

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
            Overlay::Scenes { selected } => self.paint_scenes(selected),
            Overlay::TitlePage(form) => self.paint_title_page(&form, blink_on),
        }
    }

    fn paint_selection(&mut self) {
        let Some((lo, hi)) = self.editor.selection() else {
            return;
        };
        let rows = self.text_rows();
        let left = self.left();
        for row in 0..rows {
            let Some(r) = self.layout.rows.get(self.viewport_top + row) else {
                break;
            };
            let Row::Line(li) = r else {
                continue;
            };
            let line = self.layout.lines[*li];
            let from = lo.max(line.start);
            let to = hi.min(line.end);
            for offset in from..to {
                let col = left + line.indent + (offset - line.start);
                self.screen.invert(col, wrap::TEXT_TOP + row);
            }
        }
    }

    /// Where the caret is on the grid, if it is on screen.
    fn caret_cell(&self) -> Option<(usize, usize)> {
        let (line, col) = self.cursor_position();
        let row = self.cursor_row().checked_sub(self.viewport_top)?;
        (row < self.text_rows())
            .then(|| (self.left() + self.indent(line) + col, wrap::TEXT_TOP + row))
    }

    fn paint_cursor(&mut self) {
        if let Some(cell) = self.caret_cell() {
            self.screen.set_cursor(Some(cell));
        }
    }

    /// What completion offers, in dim type after the caret.
    fn paint_completion(&mut self) {
        if !matches!(self.overlay, Overlay::None) {
            return;
        }
        let (Some(rest), Some((x, y))) = (self.completion(), self.caret_cell()) else {
            return;
        };
        let room = self.screen.cols().saturating_sub(x);
        let shown: String = rest.chars().take(room).collect();
        self.screen.put_str(x, y, &shown, DIM_FG, BG);
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
        } else if self.now_ms < self.word_count_until && self.screenplay {
            // A page of script is about a minute of screen time.
            let pages = self.layout.page_count();
            format!(
                "{pages} pp   {} scenes   ~{pages} min",
                self.layout.scenes.len()
            )
        } else if self.now_ms < self.word_count_until {
            format!("{} words", self.editor.word_count())
        } else if self.screenplay {
            format!(
                "{}   Sc {}   Pg {}/{}",
                self.caret_element().name(),
                self.layout.scene_at(self.editor.cursor()),
                self.caret_page() + 1,
                self.layout.page_count()
            )
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

/// Capitals, one character for one, so offsets survive: a letter whose
/// capital is longer (ß) is left as it is.
fn upper(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut up = c.to_uppercase();
            match (up.next(), up.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
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
            Purpose::Retrieve,
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
                Purpose::Retrieve,
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
        a.open_field(Purpose::Retrieve, "Save Document", "Filename:", "");
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
        a.open_field(Purpose::Retrieve, "Save Document", "Filename:", "ch1.txt");
        let Overlay::Field(f) = a.overlay() else {
            panic!("field");
        };
        assert_eq!(f.value(), "ch1.txt");
        assert_eq!(f.cursor(), 7, "cursor ready at the end");
    }

    #[test]
    fn submitting_a_field_hands_the_value_over_once() {
        let mut a = App::new();
        a.open_field(Purpose::Retrieve, "New Document", "Name:", "");
        key(&mut a, Command::Insert("chapter-one.txt".to_string()));
        key(&mut a, Command::Newline);
        assert!(matches!(a.overlay(), Overlay::None), "the modal closed");
        assert_eq!(
            a.take_submitted(),
            Some((Purpose::Retrieve, "chapter-one.txt".to_string()))
        );
        assert_eq!(a.take_submitted(), None, "taken only once");
    }

    #[test]
    fn escaping_a_field_submits_nothing() {
        let mut a = App::new();
        a.open_field(Purpose::Retrieve, "New Document", "Name:", "");
        key(&mut a, Command::Insert("draft".to_string()));
        key(&mut a, Command::Dismiss);
        assert!(matches!(a.overlay(), Overlay::None));
        assert_eq!(a.take_submitted(), None, "Esc skips straight past");
    }

    #[test]
    fn an_empty_name_is_the_same_as_backing_out() {
        let mut a = App::new();
        a.open_field(Purpose::Retrieve, "New Document", "Name:", "");
        key(&mut a, Command::Insert("   ".to_string()));
        key(&mut a, Command::Newline);
        assert_eq!(a.take_submitted(), None);
    }

    #[test]
    fn editing_keys_work_inside_a_field() {
        let mut a = App::new();
        a.open_field(Purpose::Retrieve, "Save Document", "Filename:", "abc");
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
            Purpose::Retrieve,
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
        a.open_field(Purpose::Retrieve, "Save Document", "Filename:", "x");
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
                let state = |a: &App| {
                    (
                        a.effects(),
                        a.dense(),
                        a.fullscreen(),
                        a.status_line(),
                        a.screenplay(),
                    )
                };
                let before = state(&a);
                key(&mut a, cmd.clone());
                let acted = a.take_deferred().is_some()
                    || !matches!(a.overlay(), Overlay::None)
                    || state(&a) != before
                    // Screenplay tools explain themselves in a prose file.
                    || a.notice.is_some()
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

#[cfg(test)]
mod screenplay_tests {
    use super::*;

    /// Type a string one key at a time, as the keyboard sends it.
    fn typed(a: &mut App, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => a.apply(Command::Newline, 0),
                '\t' => a.apply(Command::Tab, 0),
                c => a.apply(Command::Insert(c.to_string()), 0),
            }
        }
    }

    fn script() -> App {
        let mut a = App::new();
        a.start_screenplay();
        a
    }

    fn text(a: &App) -> String {
        a.editor().to_string()
    }

    /// The caret's grid cell.
    fn caret(a: &mut App) -> (usize, usize) {
        a.paint();
        a.screen().cursor().expect("cursor shown")
    }

    #[test]
    fn a_new_screenplay_opens_after_fade_in() {
        let a = script();
        assert_eq!(text(&a), "FADE IN:\n\n");
        assert_eq!(a.editor().cursor(), a.editor().len());
        assert!(!a.editor().is_dirty());
    }

    #[test]
    fn a_heading_typed_in_lower_case_comes_out_in_capitals() {
        let mut a = script();
        typed(&mut a, "int. diner - night");
        assert_eq!(text(&a), "FADE IN:\n\nINT. DINER - NIGHT");
        assert_eq!(a.caret_element(), Element::SceneHeading);
    }

    #[test]
    fn enter_moves_through_heading_action_cue_and_dialogue() {
        let mut a = script();
        typed(
            &mut a,
            "int. diner - night\nRain.\nREYES\nYou're late.\nShe sits.",
        );
        assert_eq!(
            text(&a),
            "FADE IN:\n\nINT. DINER - NIGHT\n\nRain.\n\nREYES\nYou're late.\n\nShe sits."
        );
    }

    #[test]
    fn capitals_that_end_a_sentence_stay_action() {
        let mut a = script();
        typed(&mut a, "BOOM!\nSmoke.");
        assert_eq!(text(&a), "FADE IN:\n\nBOOM!\n\nSmoke.");
    }

    #[test]
    fn the_caret_waits_at_the_dialogue_margin_under_a_cue() {
        let mut a = script();
        typed(&mut a, "REYES\n");
        assert_eq!(a.caret_element(), Element::Dialogue);
        let (col, _) = caret(&mut a);
        assert_eq!(col, script::TEXT_LEFT + 10, "dialogue at 2.5in");
    }

    #[test]
    fn tab_on_an_empty_line_makes_a_cue_and_capitalises() {
        let mut a = script();
        typed(&mut a, "\t");
        assert_eq!(a.caret_element(), Element::Character);
        let (col, _) = caret(&mut a);
        assert_eq!(col, script::TEXT_LEFT + 22, "cue at 3.7in");
        typed(&mut a, "cole\nHey.");
        assert_eq!(text(&a), "FADE IN:\n\nCOLE\nHey.");
    }

    #[test]
    fn tab_cycles_to_a_transition_and_a_forced_heading() {
        let mut a = script();
        typed(&mut a, "\t\tcut to:\n");
        assert_eq!(text(&a), "FADE IN:\n\nCUT TO:\n\n");
        typed(&mut a, "\t\t\tthe void\n");
        assert!(
            text(&a).ends_with("CUT TO:\n\n.THE VOID\n\n"),
            "{:?}",
            text(&a)
        );
        let mut b = script();
        typed(&mut b, "\t\t\t\t");
        assert_eq!(b.caret_element(), Element::Blank, "a fourth Tab goes back");
    }

    #[test]
    fn tab_in_dialogue_opens_a_parenthetical_and_enter_steps_out() {
        let mut a = script();
        typed(&mut a, "REYES\n\tquietly\nGo.");
        assert_eq!(text(&a), "FADE IN:\n\nREYES\n(quietly)\nGo.");
        let layout = &a.layout;
        assert_eq!(layout.source_kinds[3], Element::Parenthetical);
        assert_eq!(layout.source_kinds[4], Element::Dialogue);
    }

    #[test]
    fn a_cue_completes_from_names_already_used() {
        let mut a = script();
        typed(&mut a, "REYES\nHi.\n\tre");
        assert_eq!(a.completion().as_deref(), Some("YES"));
        typed(&mut a, "\n");
        assert!(text(&a).ends_with("\n\nREYES\n"), "{:?}", text(&a));
    }

    #[test]
    fn a_cue_typed_in_capitals_offers_the_name_too() {
        let mut a = script();
        typed(&mut a, "REYES\nHi.\nR");
        assert_eq!(a.completion().as_deref(), Some("EYES"));
        a.apply(
            Command::Move {
                motion: Motion::Right,
                extend: false,
            },
            0,
        );
        assert!(text(&a).ends_with("\n\nREYES"));
    }

    #[test]
    fn headings_complete_places_and_times() {
        let mut a = script();
        typed(&mut a, "int. diner - night\nRain.\next. d");
        assert_eq!(a.completion().as_deref(), Some("INER"));
        typed(&mut a, "\t - d");
        assert_eq!(a.completion().as_deref(), Some("AY"));
        typed(&mut a, "\t");
        assert!(text(&a).ends_with("EXT. DINER - DAY"));
    }

    #[test]
    fn moving_up_keeps_the_place_on_the_page() {
        let mut a = script();
        typed(
            &mut a,
            "A long line of action that runs well past the cue.\nREYES\nYes.",
        );
        // From the end of "Yes." (screen column 20 + 4) up to the cue:
        // the caret lands on the cue's own column, not four in from the
        // action margin.
        a.apply(
            Command::Move {
                motion: Motion::Up,
                extend: false,
            },
            0,
        );
        let (col, _) = caret(&mut a);
        assert_eq!(col, script::TEXT_LEFT + 22, "the cue starts right of it");
    }

    #[test]
    fn a_page_rule_is_drawn_and_clicks_on_it_land_on_text() {
        let mut a = script();
        let body: String = (0..30).map(|i| format!("Line {i}.\n")).collect();
        typed(&mut a, &body);
        let rule = a
            .layout
            .rows
            .iter()
            .position(|r| matches!(r, Row::Break(2)))
            .expect("a second page");
        a.viewport_top = rule;
        a.paint();
        let y = wrap::TEXT_TOP;
        assert_eq!(a.screen().cell(script::TEXT_LEFT, y).glyph, 0xC4);
        let offset = a.offset_at_cell(script::TEXT_LEFT, y);
        let after = a.layout.rows[rule + 1].clone();
        let Row::Line(li) = after else {
            panic!("text follows the rule");
        };
        assert_eq!(offset, a.layout.lines[li].start);
    }

    #[test]
    fn the_status_line_names_the_element_scene_and_page() {
        let mut a = script();
        typed(&mut a, "int. diner - night\nREYES\n");
        a.paint();
        let row = a.screen().rows() - 1;
        let line: String = (0..80)
            .map(|c| cp437::decode(a.screen().cell(c, row).glyph))
            .collect();
        assert!(line.contains("Dialogue   Sc 1   Pg 1/1"), "{line}");
    }

    #[test]
    fn the_print_screen_cycles_the_ribbon() {
        let mut a = script();
        a.open_print(None);
        assert_eq!(a.ribbon(), Ribbon::Fresh);
        a.apply(Command::Insert("r".to_string()), 0);
        assert_eq!(a.ribbon(), Ribbon::Worn);
        assert!(matches!(a.overlay(), Overlay::Print { .. }), "stays open");
    }

    #[test]
    fn a_screenplay_prints_as_a_pdf() {
        let mut a = script();
        typed(&mut a, "int. diner - night\nRain.");
        let pdf = a.script_pdf(Scope::Full).expect("a job");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(a.script_pdf(Scope::Page(4)).is_none(), "no such page");
    }

    #[test]
    fn the_title_page_form_writes_fountain_keys() {
        let mut a = script();
        a.apply(Command::TitlePage, 0);
        typed(&mut a, "Pilot\t\tA. Writer\t\t\t1 Main St | Springfield");
        a.apply(Command::Newline, 0);
        assert_eq!(
            text(&a),
            "Title: Pilot\nCredit: Written by\nAuthor: A. Writer\nContact:\n    1 Main St\n    Springfield\n\nFADE IN:\n\n"
        );
        assert_eq!(
            a.editor().cursor(),
            a.editor().len(),
            "the caret kept its place"
        );
        // Editing again replaces the block rather than stacking another.
        a.apply(Command::TitlePage, 0);
        a.apply(Command::Newline, 0);
        assert_eq!(text(&a).matches("Title:").count(), 1);
    }

    #[test]
    fn go_to_scene_jumps_to_a_heading() {
        let mut a = script();
        typed(&mut a, "int. one - day\nA.\next. two - night\nB.");
        a.apply(Command::SceneList, 0);
        assert_eq!(
            a.overlay(),
            &Overlay::Scenes { selected: 1 },
            "opens on the current scene"
        );
        a.apply(
            Command::Move {
                motion: Motion::Up,
                extend: false,
            },
            0,
        );
        a.apply(Command::Newline, 0);
        assert_eq!(a.editor().cursor(), "FADE IN:\n\n".len());
    }

    #[test]
    fn switching_to_prose_drops_the_indents() {
        let mut a = script();
        typed(&mut a, "REYES\nHi.");
        a.apply(Command::ToggleScreenplay, 0);
        assert!(!a.screenplay());
        assert!(a.lines().iter().all(|l| l.indent == 0));
        let (col, _) = caret(&mut a);
        assert_eq!(col, wrap::TEXT_LEFT + 3);
    }

    #[test]
    #[ignore]
    fn dump_screenplay_preview() {
        use crate::vga::{FB_HEIGHT, FB_WIDTH};
        let mut a = script();
        a.set_path(
            std::path::PathBuf::from("/Users/srhise/Documents/pilot.fountain"),
            false,
        );
        typed(
            &mut a,
            "int. fbi basement office - night\n\
A single desk lamp. Files stacked to the ceiling.\n\
REYES\n\tnot looking up\nYou're late. The tape's already rolling.\n\
\tcole (o.s.)\nThe tape's been rolling since nineteen seventy-three.\n\
\t\tcut to:\n\
ext. parking garage - continuous\n\
Concrete. Fluorescent hum.\n\
\tre",
        );
        a.paint_at(0);
        let mut fb = vec![0u8; FB_WIDTH * FB_HEIGHT * 4];
        a.screen().render(&mut fb);
        crate::vga::preview::write_bmp("target/screenplay-preview.bmp", &fb);
    }
}

#[cfg(test)]
mod screenplay_timing {
    use super::*;

    /// `cargo test --release screenplay_keystroke_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn screenplay_keystroke_cost() {
        let scene = "INT. DINER - NIGHT\n\nRain hammers the windows. A waitress refills a cup nobody asked her to refill.\n\nREYES\n(quietly)\nYou're late. The tape has been rolling since before either of us got here.\n\nCOLE\nThen we're both late.\n\n";
        let source = scene.repeat(450);
        let mut a = App::new();
        a.set_screenplay(true);
        a.load_text(&source);
        a.editor.set_cursor(source.len() / 2, false);
        a.follow_caret();
        let pages = a.page_count();
        let start = std::time::Instant::now();
        for _ in 0..20 {
            a.apply(Command::Insert("x".to_string()), 0);
            a.paint();
        }
        let per = start.elapsed() / 20;
        println!(
            "{pages} pages, {} chars: {per:?} per keystroke",
            source.len()
        );
    }
}

#[cfg(test)]
mod sound_tests {
    use super::*;

    fn typed(a: &mut App, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => a.apply(Command::Newline, 0),
                c => a.apply(Command::Insert(c.to_string()), 0),
            }
        }
    }

    fn noisy() -> App {
        let mut a = App::new();
        a.set_sounds(true);
        a
    }

    #[test]
    fn silence_unless_asked_for() {
        let mut a = App::new();
        typed(&mut a, "abc\n");
        assert!(a.take_sounds().is_empty());
    }

    #[test]
    fn keys_space_and_return_each_have_their_sound() {
        let mut a = noisy();
        typed(&mut a, "a b\n");
        assert_eq!(
            a.take_sounds(),
            vec![Sound::Key, Sound::Space, Sound::Key, Sound::Return]
        );
        assert!(a.take_sounds().is_empty(), "taken once");
        a.apply(Command::Backspace, 0);
        assert_eq!(a.take_sounds(), vec![Sound::Key]);
    }

    #[test]
    fn the_bell_rings_seven_short_of_the_margin() {
        let mut a = noisy();
        typed(&mut a, &"x".repeat(wrap::TEXT_COLS - 8));
        assert!(!a.take_sounds().contains(&Sound::Bell));
        typed(&mut a, "x");
        assert_eq!(a.take_sounds(), vec![Sound::Key, Sound::Bell]);
        typed(&mut a, "x");
        assert_eq!(a.take_sounds(), vec![Sound::Key], "once per line");
    }

    #[test]
    fn a_paste_is_not_typed() {
        let mut a = noisy();
        a.apply(Command::Insert("a whole paragraph".to_string()), 0);
        assert!(a.take_sounds().is_empty());
    }

    #[test]
    fn typing_into_a_dialog_is_silent() {
        let mut a = noisy();
        a.open_field(Purpose::Retrieve, "Retrieve Document", "Filename:", "");
        typed(&mut a, "abc");
        assert!(a.take_sounds().is_empty());
    }

    #[test]
    fn turning_sounds_off_drops_what_was_waiting() {
        let mut a = noisy();
        typed(&mut a, "ab");
        a.apply(Command::ToggleSounds, 0);
        assert!(!a.sounds_on());
        assert!(a.take_sounds().is_empty());
    }
}

#[cfg(test)]
mod greeting_tests {
    use super::*;

    fn greeted() -> App {
        let mut a = App::new();
        a.open_file_menu();
        a
    }

    #[test]
    fn the_app_opens_with_file_pulled_down() {
        let a = greeted();
        let Overlay::Menu(state) = a.overlay() else {
            panic!("the menu is up");
        };
        assert_eq!(state.menu(), 0, "File");
        assert_eq!(state.item(), Some(0), "its first item highlighted");
    }

    #[test]
    fn just_typing_goes_to_the_page() {
        let mut a = greeted();
        a.apply(Command::Insert("S".to_string()), 0);
        assert_eq!(a.overlay(), &Overlay::None);
        assert_eq!(a.editor().to_string(), "S", "not File > Save");
        assert_eq!(a.take_deferred(), None);
    }

    #[test]
    fn one_escape_puts_it_away() {
        let mut a = greeted();
        a.apply(Command::Dismiss, 0);
        assert_eq!(a.overlay(), &Overlay::None);
    }

    #[test]
    fn enter_takes_the_highlighted_item() {
        let mut a = greeted();
        a.apply(Command::Newline, 0);
        assert!(a.take_deferred().is_some());
        assert_eq!(a.editor().to_string(), "");
    }

    #[test]
    fn once_the_arrows_are_used_letters_are_shortcuts_again() {
        let mut a = greeted();
        a.apply(
            Command::Move {
                motion: Motion::Down,
                extend: false,
            },
            0,
        );
        a.apply(Command::Insert("s".to_string()), 0);
        assert_eq!(a.take_deferred(), Some(Command::Save));
        assert_eq!(a.editor().to_string(), "");
    }
}
