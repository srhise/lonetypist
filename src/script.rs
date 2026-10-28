//! A screenplay on the page: where each element sits, where the pages
//! break, and what the screen shows between them.
//!
//! The page is the typewriter's: ten characters and six lines to the
//! inch, a 1.5in left margin for the brads, 54 lines of text. The screen
//! shows paper column `c` at screen column `c - 5`, so every element
//! sits on screen exactly where it will be typed on paper.
//!
//! One paginator serves both. The pages it lays out are what prints, and
//! the rows the screen scrolls through are those same pages with a rule
//! between them, so the page number on the status line is the page
//! number on paper.

use std::collections::HashMap;

use crate::fountain::{self, Caret, Element};
use crate::status::LINES_PER_PAGE;
use crate::wrap::{self, VisualLine};

/// Paper column of the action margin: 1.5in at 10 cpi.
pub const ACTION_COL: usize = 15;
/// The 6in text block between the margins.
pub const BLOCK_WIDTH: usize = 60;
/// Paper column the right margin falls at: 7.5in.
pub const RIGHT_EDGE: usize = ACTION_COL + BLOCK_WIDTH;
/// Paper column of a character cue: 3.7in.
pub const CUE_COL: usize = 37;
/// Screen column of the action margin. Centres the block in 80 columns.
pub const TEXT_LEFT: usize = ACTION_COL - 5;

/// Times of day offered after the dash in a scene heading, before any
/// the script has used itself.
const TIMES: &[&str] = &[
    "DAY",
    "NIGHT",
    "CONTINUOUS",
    "LATER",
    "MORNING",
    "EVENING",
    "MOMENTS LATER",
    "SAME",
];

/// Where an element starts on paper, and how many columns it may run.
pub fn geometry(el: Element) -> (usize, usize) {
    match el {
        Element::Character => (CUE_COL, RIGHT_EDGE - CUE_COL),
        Element::Parenthetical => (31, 24),
        Element::Dialogue => (25, 35),
        _ => (ACTION_COL, BLOCK_WIDTH),
    }
}

/// A row of the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A visual line of the document.
    Line(usize),
    /// The rule where page `n` (one-based) begins.
    Break(usize),
    /// `(MORE)` under dialogue that runs onto the next page.
    More,
    /// The cue repeated at the top of that next page.
    Contd(String),
}

/// A line on a printed page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Line(usize),
    More,
    Contd(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    pub heading: String,
    /// Character offset of the heading's first character.
    pub offset: usize,
    /// Zero-based page.
    pub page: usize,
}

/// One line of type on a printed page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintLine {
    /// Line within the 54-line text block.
    pub row: usize,
    /// Paper column at 10 cpi.
    pub col: usize,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub lines: Vec<VisualLine>,
    /// The element of each visual line.
    pub kinds: Vec<Element>,
    /// The element of each source line.
    pub source_kinds: Vec<Element>,
    pub rows: Vec<Row>,
    pub row_of_line: Vec<usize>,
    /// Zero-based page of each visual line.
    pub page_of_line: Vec<usize>,
    pub pages: Vec<Vec<Slot>>,
    pub scenes: Vec<Scene>,
    /// Character names, most-used first.
    pub characters: Vec<String>,
    pub locations: Vec<String>,
    pub times: Vec<String>,
}

/// A visual line's text.
pub fn line_text(text: &[char], line: &VisualLine) -> String {
    text[line.start..line.end].iter().collect()
}

impl Layout {
    /// Prose: the 65-column wrap, one row per line, no breaks drawn.
    pub fn prose(text: &[char]) -> Self {
        let lines = wrap::wrap(text, wrap::TEXT_COLS);
        let n = lines.len();
        Self {
            kinds: vec![Element::Action; n],
            rows: (0..n).map(Row::Line).collect(),
            row_of_line: (0..n).collect(),
            page_of_line: (0..n).map(|i| i / LINES_PER_PAGE).collect(),
            pages: (0..n)
                .collect::<Vec<_>>()
                .chunks(LINES_PER_PAGE)
                .map(|c| c.iter().map(|&i| Slot::Line(i)).collect())
                .collect(),
            lines,
            ..Self::default()
        }
    }

    pub fn screenplay(text: &[char], caret: Caret) -> Self {
        let source: String = text.iter().collect();
        let source_kinds = fountain::classify(&source, caret);

        let mut lines: Vec<VisualLine> = Vec::new();
        let mut kinds = Vec::new();
        // The first visual line of each source line.
        let mut first_of_source = Vec::with_capacity(source_kinds.len());
        let mut start = 0usize;
        for &el in &source_kinds {
            let end = text[start..]
                .iter()
                .position(|&c| c == '\n')
                .map(|i| start + i)
                .unwrap_or(text.len());
            let (col, width) = geometry(el);
            let first = lines.len();
            first_of_source.push(first);
            wrap::wrap_paragraph(text, start, end, width, &mut lines);
            for line in &mut lines[first..] {
                let len = line.len();
                line.indent = match el {
                    Element::Transition => BLOCK_WIDTH.saturating_sub(len),
                    Element::Centered => BLOCK_WIDTH.saturating_sub(len) / 2,
                    _ => col - ACTION_COL,
                };
                kinds.push(el);
            }
            start = end + 1;
        }

        let pages = paginate(text, &lines, &kinds);
        let mut page_of_line = vec![usize::MAX; lines.len()];
        for (p, page) in pages.iter().enumerate() {
            for slot in page {
                if let Slot::Line(li) = slot {
                    page_of_line[*li] = p;
                }
            }
        }
        // Lines that never print -- notes, dropped blanks -- belong to
        // the page of whatever printed before them.
        let mut carry = 0;
        for p in page_of_line.iter_mut() {
            if *p == usize::MAX {
                *p = carry;
            } else {
                carry = *p;
            }
        }
        let (rows, row_of_line) = rows_for(&pages, lines.len());

        let mut layout = Self {
            lines,
            kinds,
            source_kinds,
            rows,
            row_of_line,
            page_of_line,
            pages,
            ..Self::default()
        };
        layout.index(text, &first_of_source, caret.line);
        layout
    }

    /// Collect the scenes and what completion offers: names, places,
    /// times. The caret's own line is left out of the names, or a cue
    /// half typed would be offered back to itself.
    fn index(&mut self, text: &[char], first_of_source: &[usize], caret_line: Option<usize>) {
        let mut names: HashMap<String, usize> = HashMap::new();
        let mut places: HashMap<String, usize> = HashMap::new();
        let mut times: HashMap<String, usize> = HashMap::new();
        for (i, &el) in self.source_kinds.iter().enumerate() {
            let li = first_of_source[i];
            let line = source_line(text, &self.lines, li);
            match el {
                Element::SceneHeading => {
                    let heading = strip_forcing(el, &line).trim().to_string();
                    self.scenes.push(Scene {
                        heading: heading.clone(),
                        offset: self.lines[li].start,
                        page: self.page_of_line[li],
                    });
                    if caret_line == Some(i) {
                        continue;
                    }
                    let body = match fountain::scene_prefix_len(&heading) {
                        Some(n) => heading[n..].trim(),
                        None => heading.as_str(),
                    };
                    let (place, time) = match body.split_once(" - ") {
                        Some((p, _)) => (p, body.rsplit(" - ").next().unwrap_or("")),
                        None => (body, ""),
                    };
                    if !place.trim().is_empty() {
                        *places.entry(place.trim().to_uppercase()).or_default() += 1;
                    }
                    if !time.trim().is_empty() {
                        *times.entry(time.trim().to_uppercase()).or_default() += 1;
                    }
                }
                Element::Character if caret_line != Some(i) => {
                    let name = fountain::cue_name(&line).to_uppercase();
                    if !name.is_empty() {
                        *names.entry(name).or_default() += 1;
                    }
                }
                _ => {}
            }
        }
        self.characters = by_use(names);
        self.locations = by_use(places);
        let mut all_times = by_use(times);
        for t in TIMES {
            if !all_times.iter().any(|x| x == t) {
                all_times.push(t.to_string());
            }
        }
        self.times = all_times;
    }

    pub fn page_count(&self) -> usize {
        self.pages.len().max(1)
    }

    /// The scene a visual line falls in: one-based, 0 before the first.
    pub fn scene_at(&self, offset: usize) -> usize {
        self.scenes.iter().filter(|s| s.offset <= offset).count()
    }

    /// The printed pages: every line of type, with its forcing marks
    /// and notes taken out and its column worked out on paper.
    pub fn print_pages(&self, text: &[char]) -> Vec<Vec<PrintLine>> {
        let cue = |s: String| PrintLine {
            row: 0,
            col: CUE_COL,
            text: s,
        };
        self.pages
            .iter()
            .map(|page| {
                let mut out = Vec::new();
                for (row, slot) in page.iter().enumerate() {
                    let mut line = match slot {
                        Slot::More => cue("(MORE)".to_string()),
                        Slot::Contd(name) => cue(name.clone()),
                        Slot::Line(li) => {
                            let el = self.kinds[*li];
                            let mut s = line_text(text, &self.lines[*li]);
                            let opens_source = *li == 0
                                || text.get(self.lines[*li].start.wrapping_sub(1)) == Some(&'\n');
                            if opens_source {
                                s = strip_forcing(el, &s);
                            }
                            let s = strip_notes(&s).trim_end().to_string();
                            let s = if el == Element::Centered || el == Element::Transition {
                                s.trim_start().to_string()
                            } else {
                                s
                            };
                            if s.trim().is_empty() {
                                continue;
                            }
                            let len = s.chars().count();
                            let col = match el {
                                Element::Transition => RIGHT_EDGE.saturating_sub(len),
                                Element::Centered => {
                                    ACTION_COL + BLOCK_WIDTH.saturating_sub(len) / 2
                                }
                                _ => geometry(el).0,
                            };
                            PrintLine {
                                row: 0,
                                col,
                                text: s,
                            }
                        }
                    };
                    line.row = row;
                    out.push(line);
                }
                out
            })
            .collect()
    }
}

fn by_use(counts: HashMap<String, usize>) -> Vec<String> {
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().map(|(s, _)| s).collect()
}

/// The whole source line a visual line begins.
fn source_line(text: &[char], lines: &[VisualLine], li: usize) -> String {
    let start = lines[li].start;
    let end = text[start..]
        .iter()
        .position(|&c| c == '\n')
        .map(|i| start + i)
        .unwrap_or(text.len());
    text[start..end].iter().collect()
}

/// Remove the character Fountain uses to force an element.
fn strip_forcing(el: Element, s: &str) -> String {
    let t = s.trim_start();
    let stripped = match el {
        Element::SceneHeading if t.starts_with('.') && !t.starts_with("..") => &t[1..],
        Element::Character if t.starts_with('@') => &t[1..],
        Element::Transition if t.starts_with('>') => &t[1..],
        Element::Action if t.starts_with('!') => &t[1..],
        Element::Centered => t.trim_end().trim_start_matches('>').trim_end_matches('<'),
        _ => return s.to_string(),
    };
    stripped.trim_start().to_string()
}

/// Take `[[notes]]` out of a line.
fn strip_notes(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find("[[") {
        out.push_str(&rest[..open]);
        match rest[open..].find("]]") {
            Some(close) => rest = &rest[open + close + 2..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether a line closes a sentence, the only place a page may split a
/// paragraph or a speech.
fn ends_sentence(s: &str) -> bool {
    let t = s.trim_end().trim_end_matches(['"', '\'', ')']);
    t.ends_with(['.', '?', '!']) || t.ends_with("--")
}

/// A stretch of lines the paginator moves as a unit, or splits by rule.
struct Block {
    kind: Element,
    lines: Vec<usize>,
}

fn blocks(lines: &[VisualLine], kinds: &[Element]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (li, &el) in kinds.iter().enumerate() {
        if !el.prints() || el == Element::TitlePage {
            continue;
        }
        let empty = lines[li].is_empty();
        let joins = |b: &Block, kind: Element| b.kind == kind && b.lines.last() == Some(&(li - 1));
        match el {
            Element::Blank => {
                // Runs of blank lines print as one.
                if out.last().is_some_and(|b| b.kind == Element::Blank) {
                    continue;
                }
                out.push(Block {
                    kind: el,
                    lines: vec![li],
                });
            }
            Element::Parenthetical | Element::Dialogue
                if out.last().is_some_and(|b| joins(b, Element::Character)) =>
            {
                out.last_mut().expect("checked").lines.push(li);
            }
            Element::Action if !empty && out.last().is_some_and(|b| joins(b, Element::Action)) => {
                out.last_mut().expect("checked").lines.push(li);
            }
            Element::Character if out.last().is_some_and(|b| joins(b, Element::Character)) => {
                // A cue that wraps onto a second line.
                out.last_mut().expect("checked").lines.push(li);
            }
            _ => {
                let kind = if el.in_dialogue() {
                    Element::Character
                } else {
                    el
                };
                out.push(Block {
                    kind,
                    lines: vec![li],
                });
            }
        }
    }
    out
}

struct Pager {
    pages: Vec<Vec<Slot>>,
}

impl Pager {
    fn used(&self) -> usize {
        self.pages.last().map(Vec::len).unwrap_or(0)
    }

    fn room(&self) -> usize {
        LINES_PER_PAGE - self.used()
    }

    fn new_page(&mut self) {
        if self.used() > 0 {
            self.pages.push(Vec::new());
        }
    }

    fn place(&mut self, slot: Slot) {
        if self.used() >= LINES_PER_PAGE {
            self.pages.push(Vec::new());
        }
        self.pages.last_mut().expect("never empty").push(slot);
    }

    fn place_lines(&mut self, lines: &[usize]) {
        for &li in lines {
            self.place(Slot::Line(li));
        }
    }
}

/// Lay the script out into pages by the rules a typist followed.
fn paginate(text: &[char], lines: &[VisualLine], kinds: &[Element]) -> Vec<Vec<Slot>> {
    let ends = |li: usize| ends_sentence(&line_text(text, &lines[li]));
    let mut pager = Pager {
        pages: vec![Vec::new()],
    };
    let blocks = blocks(lines, kinds);

    for (bi, block) in blocks.iter().enumerate() {
        let len = block.lines.len();
        match block.kind {
            Element::Blank => {
                // A page never opens on a blank line.
                if pager.used() > 0 && pager.room() > 0 {
                    pager.place(Slot::Line(block.lines[0]));
                }
            }
            Element::PageBreak => pager.new_page(),
            Element::SceneHeading => {
                // The heading, its blank line, and two lines of the
                // scene, or the heading goes over to the next page.
                let mut need = len;
                let mut next = bi + 1;
                if blocks.get(next).is_some_and(|b| b.kind == Element::Blank) {
                    need += 1;
                    next += 1;
                }
                if let Some(b) = blocks.get(next) {
                    if b.kind != Element::SceneHeading && b.kind != Element::PageBreak {
                        need += b.lines.len().min(2);
                    }
                }
                if need > pager.room() {
                    pager.new_page();
                }
                pager.place_lines(&block.lines);
            }
            Element::Action => {
                let mut rest: &[usize] = &block.lines;
                while !rest.is_empty() {
                    if rest.len() <= pager.room() {
                        pager.place_lines(rest);
                        break;
                    }
                    let room = pager.room();
                    // Two lines either side, at the end of a sentence.
                    let split = (2..=room.min(rest.len().saturating_sub(2)))
                        .rev()
                        .find(|&k| ends(rest[k - 1]));
                    let k = match split {
                        Some(k) => k,
                        // Longer than a page: it has to break somewhere.
                        None if rest.len() > LINES_PER_PAGE && room >= 2 => room,
                        None if pager.used() == 0 => room,
                        None => 0,
                    };
                    pager.place_lines(&rest[..k]);
                    pager.new_page();
                    rest = &rest[k..];
                }
            }
            Element::Character => {
                let cue =
                    fountain::cue_name(&line_text(text, &lines[block.lines[0]])).to_uppercase();
                let contd = format!("{cue} (CONT'D)");
                let mut rest: &[usize] = &block.lines;
                // Whether this stretch opens with a cue: the real one,
                // or the CONT'D placed above it on a new page.
                let mut opens_with_cue = true;
                while !rest.is_empty() {
                    if rest.len() <= pager.room() {
                        pager.place_lines(rest);
                        break;
                    }
                    // Room for (MORE) under whatever stays.
                    let room = pager.room().saturating_sub(1);
                    let min_keep = if opens_with_cue { 2 } else { 1 };
                    let speaks = |k: usize| kinds[rest[k - 1]] == Element::Dialogue;
                    let fits = min_keep..=room.min(rest.len().saturating_sub(1));
                    let split = fits
                        .clone()
                        .rev()
                        .find(|&k| speaks(k) && ends(rest[k - 1]))
                        .or_else(|| {
                            // A speech longer than a page breaks where
                            // it can, sentence or not.
                            (rest.len() >= LINES_PER_PAGE)
                                .then(|| fits.clone().rev().find(|&k| speaks(k)))
                                .flatten()
                        });
                    match split {
                        Some(k) => {
                            pager.place_lines(&rest[..k]);
                            pager.place(Slot::More);
                            pager.new_page();
                            pager.place(Slot::Contd(contd.clone()));
                            rest = &rest[k..];
                            opens_with_cue = false;
                        }
                        None if pager.used() > 0 => pager.new_page(),
                        None => {
                            pager.place_lines(rest);
                            break;
                        }
                    }
                }
            }
            _ => {
                if len > pager.room() {
                    pager.new_page();
                }
                pager.place_lines(&block.lines);
            }
        }
    }
    if pager.pages.len() > 1 && pager.pages.last().is_some_and(Vec::is_empty) {
        pager.pages.pop();
    }
    pager.pages
}

/// The screen's rows: the pages in order, a rule between each, and the
/// lines that never print slotted in where they stand in the text.
fn rows_for(pages: &[Vec<Slot>], line_count: usize) -> (Vec<Row>, Vec<usize>) {
    let mut stream: Vec<Row> = Vec::new();
    for (p, page) in pages.iter().enumerate() {
        if p > 0 {
            stream.push(Row::Break(p + 1));
        }
        for slot in page {
            stream.push(match slot {
                Slot::Line(li) => Row::Line(*li),
                Slot::More => Row::More,
                Slot::Contd(name) => Row::Contd(name.clone()),
            });
        }
    }

    let mut rows = Vec::with_capacity(line_count + pages.len() * 2);
    let mut row_of_line = vec![0; line_count];
    let mut next = 0usize;
    for (i, item) in stream.iter().enumerate() {
        // Unplaced lines go in before whatever precedes the next placed
        // line, so a blank dropped from the top of a page stays above
        // the rule rather than opening the page below it.
        let upto = match item {
            Row::Line(li) => *li,
            _ => stream[i..]
                .iter()
                .find_map(|r| match r {
                    Row::Line(li) => Some(*li),
                    _ => None,
                })
                .unwrap_or(line_count),
        };
        while next < upto {
            row_of_line[next] = rows.len();
            rows.push(Row::Line(next));
            next += 1;
        }
        if let Row::Line(li) = item {
            row_of_line[*li] = rows.len();
            next = li + 1;
        }
        rows.push(item.clone());
    }
    while next < line_count {
        row_of_line[next] = rows.len();
        rows.push(Row::Line(next));
        next += 1;
    }
    (rows, row_of_line)
}

/// The title page's fields in file order, keys lower-cased. A value that
/// runs over several indented lines keeps its line breaks.
pub fn title_page(text: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut lines = text.split('\n');
    let Some(first) = lines.next() else {
        return fields;
    };
    if fountain::title_key(first).is_none() {
        return fields;
    }
    for line in std::iter::once(first).chain(lines) {
        if line.trim().is_empty() {
            break;
        }
        match (fountain::title_key(line), line.split_once(':')) {
            (Some(key), Some((_, value))) => fields.push((key, value.trim().to_string())),
            _ => {
                if let Some((_, value)) = fields.last_mut() {
                    if !value.is_empty() {
                        value.push('\n');
                    }
                    value.push_str(line.trim());
                }
            }
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(s: &str) -> (Vec<char>, Layout) {
        let text: Vec<char> = s.chars().collect();
        let l = Layout::screenplay(&text, Caret::default());
        (text, l)
    }

    fn screen_col(l: &Layout, li: usize) -> usize {
        TEXT_LEFT + l.lines[li].indent
    }

    #[test]
    fn elements_sit_at_their_paper_columns_less_five() {
        let (_, l) =
            layout("INT. DINER - NIGHT\n\nRain.\n\nREYES\n(quietly)\nYou're late.\n\nCUT TO:");
        assert_eq!(screen_col(&l, 0), 10, "scene heading at 1.5in");
        assert_eq!(screen_col(&l, 2), 10, "action at 1.5in");
        assert_eq!(screen_col(&l, 4), 32, "cue at 3.7in");
        assert_eq!(screen_col(&l, 5), 26, "parenthetical at 3.1in");
        assert_eq!(screen_col(&l, 6), 20, "dialogue at 2.5in");
        let t = &l.lines[8];
        assert_eq!(
            TEXT_LEFT + t.indent + t.len(),
            70,
            "transition ends at 7.5in"
        );
    }

    #[test]
    fn dialogue_wraps_at_35_columns() {
        let speech = "word ".repeat(20);
        let (text, l) = layout(&format!("REYES\n{}", speech.trim()));
        for li in 1..l.lines.len() {
            assert!(line_text(&text, &l.lines[li]).chars().count() <= 35);
        }
        assert!(l.lines.len() > 3);
    }

    #[test]
    fn a_short_script_is_one_page_of_rows() {
        let (_, l) = layout("INT. A - DAY\n\nRain.");
        assert_eq!(l.page_count(), 1);
        assert_eq!(l.rows, vec![Row::Line(0), Row::Line(1), Row::Line(2)]);
    }

    fn actions(n: usize) -> String {
        (0..n)
            .map(|i| format!("Line {i}."))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    #[test]
    fn a_page_holds_54_lines_and_never_opens_on_a_blank() {
        // A two-line paragraph, then 27 more with blanks between: 56
        // lines, the 55th of them blank.
        let (_, l) = layout(&format!("Opening.\n{}", actions(28)));
        assert_eq!(l.page_count(), 2);
        assert_eq!(l.pages[0].len(), LINES_PER_PAGE);
        // Line 54 is a blank; it is dropped, and page 2 opens on text.
        assert_eq!(l.pages[1], vec![Slot::Line(55)]);
        assert!(l.rows.contains(&Row::Break(2)));
    }

    #[test]
    fn a_scene_heading_is_never_stranded() {
        // Fill to 52 lines, then a heading: it needs 1 + 1 + 2.
        let mut s = actions(26);
        s.push_str("\n\nINT. HALL - DAY\n\nShe runs.\nHe follows.");
        let (text, l) = layout(&s);
        let heading = l
            .pages
            .iter()
            .position(|p| {
                p.iter().any(|slot| {
                    matches!(slot, Slot::Line(li)
                    if line_text(&text, &l.lines[*li]).starts_with("INT."))
                })
            })
            .expect("heading placed");
        assert_eq!(heading, 1, "pushed to the next page");
    }

    #[test]
    fn a_long_speech_carries_more_and_contd() {
        let mut s = actions(25); // 49 lines
        s.push_str("\n\nREYES\n");
        let speech: Vec<String> = (0..8).map(|i| format!("Sentence number {i}.")).collect();
        s.push_str(&speech.join("\n"));
        let (text, l) = layout(&s);
        assert_eq!(l.page_count(), 2);
        assert_eq!(l.pages[0].last(), Some(&Slot::More));
        assert_eq!(l.pages[1][0], Slot::Contd("REYES (CONT'D)".to_string()));
        let pages = l.print_pages(&text);
        let more = pages[0].last().expect("more");
        assert_eq!((more.col, more.text.as_str()), (CUE_COL, "(MORE)"));
        // The rows show the same thing, with the rule between.
        let i = l
            .rows
            .iter()
            .position(|r| *r == Row::More)
            .expect("more row");
        assert_eq!(l.rows[i + 1], Row::Break(2));
        assert_eq!(l.rows[i + 2], Row::Contd("REYES (CONT'D)".to_string()));
    }

    #[test]
    fn a_speech_without_a_sentence_end_moves_whole() {
        let mut s = actions(26); // 51 lines
        s.push_str("\n\nREYES\n");
        let speech: Vec<String> = (0..4).map(|i| format!("clause {i}")).collect();
        s.push_str(&speech.join("\n"));
        let (_, l) = layout(&s);
        assert!(!l.pages[0].contains(&Slot::More));
        assert_eq!(l.pages[1].len(), 5);
    }

    #[test]
    fn page_breaks_are_forced_by_three_equals() {
        let (_, l) = layout("One.\n\n===\n\nTwo.");
        assert_eq!(l.page_count(), 2);
    }

    #[test]
    fn printing_strips_forcing_marks_and_notes() {
        let (text, l) = layout(".THE VOID\n\n@McCLANE\nYippee [[check]] ki-yay.\n\n> FADE OUT.");
        let page = &l.print_pages(&text)[0];
        let texts: Vec<&str> = page.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["THE VOID", "McCLANE", "Yippee  ki-yay.", "FADE OUT."]
        );
        let fade = page.last().expect("transition");
        assert_eq!(fade.col + fade.text.len(), RIGHT_EDGE);
    }

    #[test]
    fn notes_and_sections_do_not_print_but_do_show() {
        let (text, l) = layout("# ACT ONE\n\n[[remember]]\n\nRain.");
        assert_eq!(l.rows.len(), l.lines.len(), "every line has a row");
        let printed: Vec<String> = l.print_pages(&text)[0]
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert_eq!(printed, vec!["Rain."]);
    }

    #[test]
    fn scenes_and_completions_are_indexed() {
        let (_, l) = layout(
            "INT. DINER - NIGHT\n\nREYES\nHi.\n\nCOLE\nHey.\n\nREYES\nBye.\n\nEXT. DINER - DAY",
        );
        assert_eq!(l.scenes.len(), 2);
        assert_eq!(l.scenes[1].heading, "EXT. DINER - DAY");
        assert_eq!(l.characters, vec!["REYES", "COLE"]);
        assert_eq!(l.locations, vec!["DINER"]);
        assert!(l
            .times
            .starts_with(&["DAY".to_string(), "NIGHT".to_string()]));
    }

    #[test]
    fn the_title_page_is_read_with_continuations() {
        let fields =
            title_page("Title: PILOT\nContact:\n    1 Main St\n    Springfield\n\nFADE IN:");
        assert_eq!(fields[0], ("title".to_string(), "PILOT".to_string()));
        assert_eq!(
            fields[1],
            ("contact".to_string(), "1 Main St\nSpringfield".to_string())
        );
    }
}
