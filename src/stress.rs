//! A randomized driver over the command surface.
//!
//! The app crashed inside AppKit's event dispatch, which means a panic in
//! command handling or painting. This hammers those paths with pseudo-random
//! sequences so the panic can be reproduced without a window.

#![cfg(test)]

use crate::app::App;
use crate::keymap::{Command, Motion};

/// Deterministic LCG so a failing seed can be replayed exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const MOTIONS: [Motion; 12] = [
    Motion::Left,
    Motion::Right,
    Motion::Up,
    Motion::Down,
    Motion::WordLeft,
    Motion::WordRight,
    Motion::LineStart,
    Motion::LineEnd,
    Motion::DocStart,
    Motion::DocEnd,
    Motion::PageUp,
    Motion::PageDown,
];

/// Words chosen to exercise wrapping: some longer than the 65-column
/// measure, some with characters the input filter rewrites or rejects.
const WORDS: [&str; 10] = [
    "the",
    "quick",
    "a",
    "  ",
    "supercalifragilisticexpialidociousandthensomemoreletters",
    "\u{201C}quoted\u{201D}",
    "caf\u{00E9}",
    "\u{3042}\u{3044}",
    "x",
    "well-turned",
];

fn random_command(rng: &mut Rng) -> Command {
    match rng.below(15) {
        0..=2 => Command::Insert(WORDS[rng.below(WORDS.len() as u64) as usize].to_string()),
        3 => Command::Newline,
        4 => Command::Tab,
        5 => Command::Backspace,
        6 => Command::DeleteForward,
        7..=8 => Command::Move {
            motion: MOTIONS[rng.below(MOTIONS.len() as u64) as usize],
            extend: rng.below(2) == 0,
        },
        9 => Command::SelectAll,
        10 => Command::Undo,
        11 => Command::Redo,
        12 => Command::ToggleDenseMode,
        13 => Command::MenuBar,
        _ => Command::ShowWordCount,
    }
}

#[test]
fn random_command_sequences_never_panic() {
    for seed in 0..1500u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut app = App::new();
        for step in 0..120u64 {
            let now = step * 37;
            // Drop into a modal field now and then, so its key routing
            // is exercised in the middle of arbitrary sequences.
            if rng.below(40) == 0 {
                app.open_field(
                    crate::input::Purpose::Retrieve,
                    "Save Document",
                    "Filename:",
                    "seed.txt",
                );
            }
            let _ = app.take_submitted();
            match rng.below(10) {
                // Occasionally poke the mouse paths too.
                8 => {
                    let col = rng.below(80) as usize;
                    let row = rng.below(50) as usize;
                    app.click(col, row, rng.below(2) == 0);
                }
                9 => {
                    let delta = rng.below(21) as i32 - 10;
                    app.scroll(delta);
                }
                _ => app.apply(random_command(&mut rng), now),
            }
            // Painting is what the window does on every event.
            app.paint_at(now);
        }
    }
}

#[test]
fn random_sequences_never_panic_in_dense_mode() {
    for seed in 0..600u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        let mut app = App::new();
        app.set_dense(true);
        for step in 0..120u64 {
            app.apply(random_command(&mut rng), step * 37);
            app.paint_at(step * 37);
        }
    }
}
