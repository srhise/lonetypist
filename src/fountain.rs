//! Reading Fountain: which screenplay element each line of the source is.
//!
//! Fountain marks nothing up that a writer would not type anyway. A line
//! is a scene heading because it starts INT., a character cue because it
//! is in capitals with dialogue under it. Most of that is decided by the
//! neighbouring lines, which is the one place an editor differs from a
//! file: the line the caret is on may be empty only because the writer
//! has not typed it yet. So the caller names that line, and it counts as
//! content -- a cue with the caret waiting below it is a cue.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Element {
    Blank,
    TitlePage,
    SceneHeading,
    Action,
    Character,
    Parenthetical,
    Dialogue,
    Transition,
    Centered,
    PageBreak,
    Section,
    Synopsis,
    Note,
    Boneyard,
}

impl Element {
    /// The name the status line shows.
    pub fn name(self) -> &'static str {
        match self {
            Element::Blank | Element::Action => "Action",
            Element::TitlePage => "Title Page",
            Element::SceneHeading => "Scene Heading",
            Element::Character => "Character",
            Element::Parenthetical => "Parenthetical",
            Element::Dialogue => "Dialogue",
            Element::Transition => "Transition",
            Element::Centered => "Centered",
            Element::PageBreak => "Page Break",
            Element::Section => "Section",
            Element::Synopsis => "Synopsis",
            Element::Note => "Note",
            Element::Boneyard => "Boneyard",
        }
    }

    /// Part of a speech: a cue and everything said under it.
    pub fn in_dialogue(self) -> bool {
        matches!(
            self,
            Element::Character | Element::Parenthetical | Element::Dialogue
        )
    }

    /// Elements that are written in capitals.
    pub fn capitalised(self) -> bool {
        matches!(
            self,
            Element::SceneHeading | Element::Character | Element::Transition
        )
    }

    /// Whether the element reaches paper at all.
    pub fn prints(self) -> bool {
        !matches!(
            self,
            Element::Section | Element::Synopsis | Element::Note | Element::Boneyard
        )
    }
}

/// Context for classifying an editor's buffer rather than a file.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    /// The source line the caret is on.
    pub line: Option<usize>,
    /// An element the writer chose with Tab for that line.
    pub pending: Option<Element>,
}

const TITLE_KEYS: &[&str] = &[
    "title",
    "credit",
    "author",
    "authors",
    "source",
    "draft date",
    "date",
    "contact",
    "copyright",
    "notes",
    "revision",
];

const SCENE_PREFIXES: &[&str] = &[
    "INT./EXT.",
    "INT/EXT.",
    "INT./EXT ",
    "INT/EXT ",
    "I/E.",
    "I/E ",
    "INT.",
    "EXT.",
    "EST.",
    "INT ",
    "EXT ",
    "EST ",
];

/// A title page key at the start of `line`, lower-cased, if there is one.
pub fn title_key(line: &str) -> Option<String> {
    let (key, _) = line.split_once(':')?;
    let key = key.trim().to_lowercase();
    TITLE_KEYS.contains(&key.as_str()).then_some(key)
}

/// Whether a line opens with one of the scene heading prefixes.
pub fn is_scene_heading(line: &str) -> bool {
    let upper = line.trim_start().to_uppercase();
    SCENE_PREFIXES.iter().any(|p| upper.starts_with(p))
}

/// The prefix of a scene heading, `INT. ` and the like, as typed.
pub fn scene_prefix_len(line: &str) -> Option<usize> {
    let upper = line.to_uppercase();
    SCENE_PREFIXES
        .iter()
        .find(|p| upper.starts_with(*p))
        .map(|p| p.len())
}

/// A character cue's name in capitals, with any extension like `(V.O.)`
/// allowed to be in any case. It has to hold at least one letter.
pub fn is_cue(line: &str) -> bool {
    let name = cue_name(line);
    name.chars().any(|c| c.is_alphabetic()) && !name.chars().any(|c| c.is_lowercase())
}

/// The name in a cue, without extensions or the dual-dialogue caret.
pub fn cue_name(line: &str) -> &str {
    let mut s = line
        .trim()
        .trim_start_matches('@')
        .trim_end_matches('^')
        .trim_end();
    while s.ends_with(')') {
        match s.rfind('(') {
            Some(open) => s = s[..open].trim_end(),
            None => break,
        }
    }
    s
}

fn is_transition(line: &str) -> bool {
    let t = line.trim();
    t.ends_with("TO:") && !t.chars().any(|c| c.is_lowercase())
}

/// Classify every line of `text`, split on `\n`.
pub fn classify(text: &str, caret: Caret) -> Vec<Element> {
    let lines: Vec<&str> = text.split('\n').collect();
    let n = lines.len();
    let blank = |i: usize| lines[i].trim().is_empty();
    // A line holds something, or the caret is there to type it.
    let filled = |i: usize| i < n && (!blank(i) || caret.line == Some(i));

    let mut out = vec![Element::Action; n];
    let mut i = 0;

    // The title page: key/value lines from the very top, to a blank line.
    if n > 0 && title_key(lines[0]).is_some() {
        while i < n && !blank(i) {
            out[i] = Element::TitlePage;
            i += 1;
        }
    }

    let mut in_boneyard = false;
    while i < n {
        let line = lines[i];
        let t = line.trim();
        let prev = if i == 0 { None } else { Some(out[i - 1]) };
        let after_blank = i == 0 || blank(i - 1) || prev == Some(Element::TitlePage);
        let pending = if caret.line == Some(i) {
            caret.pending
        } else {
            None
        };

        let el = if in_boneyard || t.starts_with("/*") {
            in_boneyard = !t.contains("*/");
            Element::Boneyard
        } else if t.is_empty() {
            match (pending, prev) {
                (Some(p), _) => p,
                (None, Some(p)) if p.in_dialogue() && caret.line == Some(i) && !blank(i - 1) => {
                    Element::Dialogue
                }
                _ => Element::Blank,
            }
        } else if prev.is_some_and(|p| p.in_dialogue()) && !blank(i - 1) {
            if t.starts_with('(') {
                Element::Parenthetical
            } else {
                Element::Dialogue
            }
        } else if let Some(p) = pending {
            p
        } else if t.len() >= 3 && t.chars().all(|c| c == '=') {
            Element::PageBreak
        } else if t.starts_with("[[") && t.ends_with("]]") {
            Element::Note
        } else if t.starts_with('#') {
            Element::Section
        } else if t.starts_with('=') {
            Element::Synopsis
        } else if t.starts_with('>') && t.ends_with('<') {
            Element::Centered
        } else if t.starts_with('!') || t.starts_with('~') || !after_blank {
            // Forced action, lyrics, and anything not opening a block.
            Element::Action
        } else if t.starts_with('.') && !t.starts_with("..") {
            Element::SceneHeading
        } else if t.starts_with('@') {
            Element::Character
        } else if t.starts_with('>') {
            Element::Transition
        } else if is_scene_heading(t) {
            Element::SceneHeading
        } else if is_transition(t) {
            Element::Transition
        } else if is_cue(t) && filled(i + 1) {
            Element::Character
        } else {
            Element::Action
        };
        out[i] = el;
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Element::*;

    fn kinds(text: &str) -> Vec<Element> {
        classify(text, Caret::default())
    }

    #[test]
    fn a_scene_heading_needs_a_prefix_after_a_blank_line() {
        assert_eq!(kinds("INT. KITCHEN - DAY"), vec![SceneHeading]);
        assert_eq!(kinds("ext. pier - night"), vec![SceneHeading]);
        assert_eq!(kinds("I/E. CAR - MOVING"), vec![SceneHeading]);
        assert_eq!(kinds("Interior design"), vec![Action]);
        assert_eq!(kinds("He walks.\nINT. HALL"), vec![Action, Action]);
    }

    #[test]
    fn a_dot_forces_a_heading_but_an_ellipsis_does_not() {
        assert_eq!(kinds(".THE VOID"), vec![SceneHeading]);
        assert_eq!(kinds("...and then"), vec![Action]);
    }

    #[test]
    fn a_cue_is_capitals_with_dialogue_under_it() {
        assert_eq!(
            kinds("REYES\nYou're late.\n\nShe sits."),
            vec![Character, Dialogue, Blank, Action]
        );
    }

    #[test]
    fn capitals_without_dialogue_are_action() {
        assert_eq!(kinds("BANG!\n\nSilence."), vec![Action, Blank, Action]);
    }

    #[test]
    fn extensions_may_be_lower_case() {
        assert_eq!(kinds("REYES (cont'd)\nAgain.")[0], Character);
        assert_eq!(kinds("MULDOON (V.O.)\nHello.")[0], Character);
        assert_eq!(kinds("McCLANE\nYippee.")[0], Action, "unforced mixed case");
        assert_eq!(kinds("@McCLANE\nYippee.")[0], Character);
    }

    #[test]
    fn parentheticals_and_dialogue_follow_a_cue() {
        assert_eq!(
            kinds("REYES\n(quietly)\nGo.\n(beat)\nNow."),
            vec![Character, Parenthetical, Dialogue, Parenthetical, Dialogue]
        );
    }

    #[test]
    fn transitions_end_in_to_and_stand_alone() {
        assert_eq!(kinds("Done.\n\nCUT TO:\n\nINT. X")[2], Transition);
        assert_eq!(kinds("> FADE OUT.")[0], Transition);
        assert_eq!(kinds(">THE END<")[0], Centered);
    }

    #[test]
    fn the_title_page_runs_to_the_first_blank_line() {
        assert_eq!(
            kinds("Title: PILOT\nAuthor: Me\n\nFADE IN:"),
            vec![TitlePage, TitlePage, Blank, Action]
        );
        assert_eq!(kinds("Note: not a key")[0], Action);
    }

    #[test]
    fn things_that_never_print() {
        assert_eq!(kinds("# ACT ONE")[0], Section);
        assert_eq!(kinds("= She finds the tape.")[0], Synopsis);
        assert_eq!(kinds("[[check this]]")[0], Note);
        assert_eq!(kinds("===")[0], PageBreak);
        assert_eq!(
            kinds("/* cut\nthis scene */\nKept."),
            vec![Boneyard, Boneyard, Action]
        );
    }

    #[test]
    fn the_caret_line_counts_as_content() {
        // A cue with nothing under it yet, and the caret on the empty
        // line waiting for the dialogue.
        let text = "Rain.\n\nREYES\n";
        assert_eq!(kinds(text)[2], Action, "on disk this is not yet a cue");
        let caret = Caret {
            line: Some(3),
            pending: None,
        };
        assert_eq!(
            classify(text, caret),
            vec![Action, Blank, Character, Dialogue]
        );
    }

    #[test]
    fn a_capital_at_the_start_of_action_does_not_jump_to_a_cue() {
        let caret = Caret {
            line: Some(2),
            pending: None,
        };
        assert_eq!(classify("Rain.\n\nT", caret)[2], Action);
    }

    #[test]
    fn a_pending_element_holds_the_caret_line() {
        let caret = Caret {
            line: Some(2),
            pending: Some(Character),
        };
        assert_eq!(classify("Rain.\n\n", caret)[2], Character);
        assert_eq!(classify("Rain.\n\nRE", caret)[2], Character);
    }

    #[test]
    fn cue_names_lose_their_extensions() {
        assert_eq!(cue_name("REYES (V.O.) (CONT'D)"), "REYES");
        assert_eq!(cue_name("@McCLANE ^"), "McCLANE");
    }
}
