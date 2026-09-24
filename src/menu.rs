//! The Alt-= menu bar.
//!
//! The tree is a static table: every item carries the command it emits
//! and the hotkey to show beside it, so the menu teaches the shortcuts
//! rather than competing with them.

use crate::keymap::Command;

pub struct Item {
    pub label: &'static str,
    pub hotkey: &'static str,
    /// `None` marks a separator rule.
    pub command: Option<Command>,
}

pub struct Menu {
    pub title: &'static str,
    pub items: &'static [Item],
}

const fn sep() -> Item {
    Item {
        label: "",
        hotkey: "",
        command: None,
    }
}

const fn item(label: &'static str, hotkey: &'static str, command: Command) -> Item {
    Item {
        label,
        hotkey,
        command: Some(command),
    }
}

static FILE_ITEMS: &[Item] = &[
    item("Retrieve...", "Shft-F10", Command::Retrieve),
    item("Open (browse)...", "Cmd-O", Command::Open),
    item("Save", "Cmd-S", Command::Save),
    item("Save As...", "F10", Command::SaveAs),
    item("New", "Cmd-N", Command::New),
    sep(),
    item("Print...", "Shft-F7", Command::Print),
    sep(),
    item("Exit", "F7", Command::Quit),
];

static EDIT_ITEMS: &[Item] = &[
    item("Undo", "Cmd-Z", Command::Undo),
    item("Redo", "Cmd-Shft-Z", Command::Redo),
    sep(),
    item("Cut", "Cmd-X", Command::Cut),
    item("Copy", "Cmd-C", Command::Copy),
    item("Paste", "Cmd-V", Command::Paste),
    sep(),
    item("All (select)", "Cmd-A", Command::SelectAll),
];

static VIEW_ITEMS: &[Item] = &[
    item("CRT Effects", "F3", Command::ToggleEffects),
    item("Screen Mode", "F5", Command::ToggleDenseMode),
    item("Status Line", "F4", Command::ToggleStatusLine),
    item("Full Screen", "F11", Command::ToggleFullscreen),
];

static TOOLS_ITEMS: &[Item] = &[item("Word Count", "F6", Command::ShowWordCount)];

static HELP_ITEMS: &[Item] = &[item("Help", "Shft-F1", Command::ToggleHelp)];

pub static MENUS: &[Menu] = &[
    Menu {
        title: "File",
        items: FILE_ITEMS,
    },
    Menu {
        title: "Edit",
        items: EDIT_ITEMS,
    },
    Menu {
        title: "View",
        items: VIEW_ITEMS,
    },
    Menu {
        title: "Tools",
        items: TOOLS_ITEMS,
    },
    Menu {
        title: "Help",
        items: HELP_ITEMS,
    },
];

/// Where the highlight is. `item` is `None` while only the bar is
/// highlighted and no dropdown has been pulled down yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuState {
    menu: usize,
    item: Option<usize>,
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            menu: 0,
            item: None,
        }
    }

    pub fn menu(&self) -> usize {
        self.menu
    }

    pub fn item(&self) -> Option<usize> {
        self.item
    }

    fn items(&self) -> &'static [Item] {
        MENUS[self.menu].items
    }

    fn first_selectable(&self) -> Option<usize> {
        self.items().iter().position(|i| i.command.is_some())
    }

    fn last_selectable(&self) -> Option<usize> {
        self.items().iter().rposition(|i| i.command.is_some())
    }

    /// Step through the items, skipping separators and wrapping.
    fn step(&self, from: usize, forward: bool) -> Option<usize> {
        let n = self.items().len();
        if n == 0 {
            return None;
        }
        let mut i = from;
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if self.items()[i].command.is_some() {
                return Some(i);
            }
        }
        None
    }

    /// Moving along the bar re-opens at the new menu's first item, so the
    /// dropdown follows you rather than snapping shut.
    fn slide(&mut self, forward: bool) {
        let n = MENUS.len();
        self.menu = if forward {
            (self.menu + 1) % n
        } else {
            (self.menu + n - 1) % n
        };
        if self.item.is_some() {
            self.item = self.first_selectable();
        }
    }

    pub fn right(&mut self) {
        self.slide(true);
    }

    pub fn left(&mut self) {
        self.slide(false);
    }

    pub fn down(&mut self) {
        self.item = match self.item {
            None => self.first_selectable(),
            Some(i) => self.step(i, true),
        };
    }

    pub fn up(&mut self) {
        self.item = match self.item {
            None => self.last_selectable(),
            Some(i) => self.step(i, false),
        };
    }

    /// Enter: open the dropdown, or fire the highlighted item.
    pub fn activate(&mut self) -> Option<Command> {
        match self.item {
            None => {
                self.item = self.first_selectable();
                None
            }
            Some(i) => self.items()[i].command.clone(),
        }
    }

    /// A letter jumps to a menu when only the bar is up, or fires the
    /// matching item when a dropdown is open.
    pub fn letter(&mut self, c: char) -> Option<Command> {
        let c = c.to_ascii_lowercase();
        match self.item {
            None => {
                let found = MENUS.iter().position(|m| {
                    m.title
                        .chars()
                        .next()
                        .map(|t| t.to_ascii_lowercase() == c)
                        .unwrap_or(false)
                });
                if let Some(m) = found {
                    self.menu = m;
                    self.item = self.first_selectable();
                }
                None
            }
            Some(_) => {
                let found = self.items().iter().position(|i| {
                    i.command.is_some()
                        && i.label
                            .chars()
                            .next()
                            .map(|t| t.to_ascii_lowercase() == c)
                            .unwrap_or(false)
                });
                match found {
                    Some(i) => {
                        self.item = Some(i);
                        self.items()[i].command.clone()
                    }
                    None => None,
                }
            }
        }
    }

    /// Returns whether the menu is still open afterwards.
    pub fn escape(&mut self) -> bool {
        if self.item.is_some() {
            self.item = None;
            true
        } else {
            false
        }
    }
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Command;

    fn open() -> MenuState {
        MenuState::new()
    }

    #[test]
    fn opens_on_the_first_menu_with_no_dropdown() {
        let m = open();
        assert_eq!(m.menu(), 0);
        assert_eq!(m.item(), None, "the bar is highlighted, nothing dropped");
    }

    #[test]
    fn every_menu_has_a_title_and_at_least_one_command() {
        for menu in MENUS {
            assert!(!menu.title.is_empty());
            assert!(
                menu.items.iter().any(|i| i.command.is_some()),
                "{}",
                menu.title
            );
        }
    }

    #[test]
    fn moving_right_walks_the_bar_and_wraps() {
        let mut m = open();
        m.right();
        assert_eq!(m.menu(), 1);
        for _ in 0..MENUS.len() {
            m.right();
        }
        assert_eq!(m.menu(), 1, "wrapped all the way round");
    }

    #[test]
    fn moving_left_from_the_first_menu_wraps_to_the_last() {
        let mut m = open();
        m.left();
        assert_eq!(m.menu(), MENUS.len() - 1);
    }

    #[test]
    fn moving_along_the_bar_closes_an_open_dropdown() {
        let mut m = open();
        m.down();
        assert!(m.item().is_some());
        m.right();
        assert_eq!(m.item(), Some(0), "the next menu opens at its first item");
    }

    #[test]
    fn down_opens_the_dropdown_at_the_first_item() {
        let mut m = open();
        m.down();
        assert_eq!(m.item(), Some(0));
    }

    #[test]
    fn down_skips_separators() {
        // File: ... then a separator, then Exit.
        let mut m = open();
        m.down();
        let mut seen = Vec::new();
        for _ in 0..MENUS[0].items.len() {
            seen.push(m.item().expect("dropdown open"));
            m.down();
        }
        for i in seen {
            assert!(
                MENUS[0].items[i].command.is_some(),
                "landed on a separator at {i}"
            );
        }
    }

    #[test]
    fn item_movement_wraps_within_the_menu() {
        let mut m = open();
        m.down();
        let first = m.item().expect("open");
        // Step once per *selectable* item: separators are skipped, so
        // counting the raw entries would overshoot the wrap.
        let selectable = MENUS[0]
            .items
            .iter()
            .filter(|i| i.command.is_some())
            .count();
        for _ in 0..selectable {
            m.down();
        }
        assert_eq!(m.item(), Some(first), "wrapped back to the first item");
    }

    #[test]
    fn up_from_the_first_item_wraps_to_the_last_selectable() {
        let mut m = open();
        m.down();
        m.up();
        let i = m.item().expect("open");
        assert!(MENUS[0].items[i].command.is_some());
        let last_selectable = MENUS[0]
            .items
            .iter()
            .rposition(|it| it.command.is_some())
            .expect("a command");
        assert_eq!(i, last_selectable);
    }

    #[test]
    fn enter_on_the_bar_opens_the_dropdown_rather_than_firing() {
        let mut m = open();
        assert_eq!(m.activate(), None);
        assert_eq!(m.item(), Some(0));
    }

    #[test]
    fn enter_on_an_item_returns_its_command() {
        let mut m = open();
        m.down();
        let cmd = m.activate();
        assert_eq!(cmd, MENUS[0].items[0].command.clone());
        assert!(cmd.is_some());
    }

    #[test]
    fn a_letter_on_the_bar_jumps_to_that_menu_and_opens_it() {
        let mut m = open();
        assert_eq!(m.letter('e'), None, "opens Edit, fires nothing");
        assert_eq!(MENUS[m.menu()].title, "Edit");
        assert_eq!(m.item(), Some(0));
    }

    #[test]
    fn a_letter_in_a_dropdown_fires_the_matching_item() {
        let mut m = open();
        m.letter('e'); // Edit
        let cmd = m.letter('u'); // Undo
        assert_eq!(cmd, Some(Command::Undo));
    }

    #[test]
    fn an_unmatched_letter_does_nothing() {
        let mut m = open();
        m.down();
        let before = m.item();
        assert_eq!(m.letter('z'), None);
        assert_eq!(m.item(), before);
    }

    #[test]
    fn letters_match_case_insensitively() {
        let mut m = open();
        assert_eq!(m.letter('E'), None);
        assert_eq!(MENUS[m.menu()].title, "Edit");
    }

    #[test]
    fn escape_closes_the_dropdown_first_then_the_bar() {
        let mut m = open();
        m.down();
        assert!(m.escape(), "still in the menu, dropdown closed");
        assert_eq!(m.item(), None);
        assert!(!m.escape(), "second escape leaves the menu");
    }

    #[test]
    fn every_item_with_a_command_has_a_label() {
        for menu in MENUS {
            for item in menu.items {
                if item.command.is_some() {
                    assert!(!item.label.is_empty(), "{} has a blank item", menu.title);
                }
            }
        }
    }

    #[test]
    fn separators_carry_no_label_or_hotkey() {
        for menu in MENUS {
            for item in menu.items {
                if item.command.is_none() {
                    assert!(item.label.is_empty());
                    assert!(item.hotkey.is_empty());
                }
            }
        }
    }

    #[test]
    fn menu_titles_start_with_distinct_letters() {
        // Otherwise the letter shortcut on the bar is ambiguous.
        let mut initials: Vec<char> = MENUS
            .iter()
            .map(|m| m.title.chars().next().expect("title").to_ascii_lowercase())
            .collect();
        initials.sort_unstable();
        let before = initials.len();
        initials.dedup();
        assert_eq!(initials.len(), before, "two menus share an initial");
    }
}
