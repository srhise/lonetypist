//! Translating key events into commands. DOS look, modern muscle memory:
//! Cmd-S saves, Cmd-Z undoes, the arrows behave as expected.

use winit::event::{KeyEvent, Modifiers};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    PageUp,
    PageDown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Insert(String),
    Newline,
    Tab,
    Backspace,
    DeleteForward,
    Move {
        motion: Motion,
        extend: bool,
    },
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    /// In-world open: type a name rather than browsing.
    Retrieve,
    /// The Shift-F7 print screen.
    Print,
    /// Drop the Alt-= menu bar.
    MenuBar,
    ToggleHelp,
    ToggleEffects,
    ToggleDenseMode,
    ShowWordCount,
    ToggleFullscreen,
    Dismiss,
}

pub fn resolve(event: &KeyEvent, mods: &Modifiers) -> Option<Command> {
    let key = &event.logical_key;
    let m: ModifiersState = mods.state();
    let cmd = m.super_key();
    let shift = m.shift_key();
    let alt = m.alt_key();

    // Alt-= drops the menu bar. Matched on the physical key because
    // Option-= produces a different character on macOS.
    if alt && event.physical_key == PhysicalKey::Code(KeyCode::Equal) {
        return Some(Command::MenuBar);
    }

    // Command-key bindings first: they never insert text.
    if cmd {
        if let Key::Character(c) = key {
            return match c.to_lowercase().as_str() {
                "a" => Some(Command::SelectAll),
                "c" => Some(Command::Copy),
                "x" => Some(Command::Cut),
                "v" => Some(Command::Paste),
                "z" if shift => Some(Command::Redo),
                "z" => Some(Command::Undo),
                "s" if shift => Some(Command::SaveAs),
                "s" => Some(Command::Save),
                "o" => Some(Command::Open),
                "n" => Some(Command::New),
                "p" => Some(Command::Print),
                "q" => Some(Command::Quit),
                _ => None,
            };
        }
    }

    let motion = |motion| {
        Some(Command::Move {
            motion,
            extend: shift,
        })
    };

    match key {
        Key::Named(NamedKey::ArrowLeft) if cmd => motion(Motion::LineStart),
        Key::Named(NamedKey::ArrowRight) if cmd => motion(Motion::LineEnd),
        Key::Named(NamedKey::ArrowUp) if cmd => motion(Motion::DocStart),
        Key::Named(NamedKey::ArrowDown) if cmd => motion(Motion::DocEnd),
        Key::Named(NamedKey::ArrowLeft) if alt => motion(Motion::WordLeft),
        Key::Named(NamedKey::ArrowRight) if alt => motion(Motion::WordRight),
        Key::Named(NamedKey::ArrowLeft) => motion(Motion::Left),
        Key::Named(NamedKey::ArrowRight) => motion(Motion::Right),
        Key::Named(NamedKey::ArrowUp) => motion(Motion::Up),
        Key::Named(NamedKey::ArrowDown) => motion(Motion::Down),
        Key::Named(NamedKey::Home) => motion(Motion::LineStart),
        Key::Named(NamedKey::End) => motion(Motion::LineEnd),
        Key::Named(NamedKey::PageUp) => motion(Motion::PageUp),
        Key::Named(NamedKey::PageDown) => motion(Motion::PageDown),

        Key::Named(NamedKey::Enter) => Some(Command::Newline),
        Key::Named(NamedKey::Tab) => Some(Command::Tab),
        Key::Named(NamedKey::Backspace) => Some(Command::Backspace),
        Key::Named(NamedKey::Delete) => Some(Command::DeleteForward),
        Key::Named(NamedKey::Escape) => Some(Command::Dismiss),
        Key::Named(NamedKey::Space) => Some(Command::Insert(" ".to_string())),

        // F1 is the menu; Help moves one key over. Both are also in
        // the Help menu, so neither is the only way in.
        Key::Named(NamedKey::F1) if shift => Some(Command::ToggleHelp),
        Key::Named(NamedKey::F1) => Some(Command::MenuBar),
        // WordPerfect's own: F7 leaves, Shift-F7 prints, F10 saves,
        // Shift-F10 retrieves.
        Key::Named(NamedKey::F7) if shift => Some(Command::Print),
        Key::Named(NamedKey::F7) => Some(Command::Quit),
        Key::Named(NamedKey::F10) if shift => Some(Command::Retrieve),
        Key::Named(NamedKey::F10) => Some(Command::SaveAs),
        Key::Named(NamedKey::F3) => Some(Command::ToggleEffects),
        Key::Named(NamedKey::F5) => Some(Command::ToggleDenseMode),
        Key::Named(NamedKey::F6) => Some(Command::ShowWordCount),
        Key::Named(NamedKey::F11) => Some(Command::ToggleFullscreen),

        Key::Character(text) => Some(Command::Insert(text.to_string())),
        _ => None,
    }
}
