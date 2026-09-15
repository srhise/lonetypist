//! Printing the way a line printer took it: plain text, ten characters
//! to the inch, six lines to the inch, a form feed between pages. The
//! page geometry is the same one the status line reports, so "Pg 3"
//! on screen is page 3 on paper.

use crate::status::LINES_PER_PAGE;
use crate::wrap::VisualLine;

/// One inch of left margin at 10 cpi.
const LEFT_MARGIN: usize = 10;
/// One inch of top margin at 6 lpi.
const TOP_MARGIN: usize = 6;
const FORM_FEED: char = '\x0c';

/// What the Print screen was asked for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Everything, in order.
    Full,
    /// A single zero-based page.
    Page(usize),
}

/// Where the job goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Printer(Scope),
    /// Written to disk as the printer would have received it.
    ToFile(Scope),
}

/// The zero-based page a visual line falls on.
pub fn page_of(visual_line: usize) -> usize {
    visual_line / LINES_PER_PAGE
}

/// How many pages the document occupies. An empty document is still
/// one page, as the status line says.
pub fn page_count(lines: &[VisualLine]) -> usize {
    lines.len().div_ceil(LINES_PER_PAGE).max(1)
}

/// Split the wrapped document into pages of at most `LINES_PER_PAGE`
/// lines each, with trailing whitespace trimmed so a blank line is
/// really blank.
pub fn paginate(text: &[char], lines: &[VisualLine]) -> Vec<Vec<String>> {
    let mut pages: Vec<Vec<String>> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i % LINES_PER_PAGE == 0 {
            pages.push(Vec::new());
        }
        let s: String = text[line.start..line.end].iter().collect();
        pages
            .last_mut()
            .expect("a page was pushed")
            .push(s.trim_end().to_string());
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    pages
}

/// Render pages for the printer. Every page opens with the top margin,
/// every line carries the left margin, and a form feed separates pages.
/// There is no trailing form feed: the printer ejects the last page on
/// its own, and a stray one would waste a sheet.
pub fn render(pages: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push(FORM_FEED);
        }
        for _ in 0..TOP_MARGIN {
            out.push('\n');
        }
        for line in page {
            if !line.is_empty() {
                out.push_str(&" ".repeat(LEFT_MARGIN));
                out.push_str(line);
            }
            out.push('\n');
        }
    }
    out
}

/// The text a job sends to the printer, or `None` when the requested
/// page does not exist.
pub fn job(text: &[char], lines: &[VisualLine], scope: Scope) -> Option<String> {
    let pages = paginate(text, lines);
    match scope {
        Scope::Full => Some(render(&pages)),
        Scope::Page(n) => pages.get(n).map(|p| render(std::slice::from_ref(p))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap;

    fn wrapped(s: &str) -> (Vec<char>, Vec<VisualLine>) {
        let text: Vec<char> = s.chars().collect();
        let lines = wrap::wrap(&text, wrap::TEXT_COLS);
        (text, lines)
    }

    #[test]
    fn a_short_document_is_one_page() {
        let (t, l) = wrapped("hello\nworld");
        assert_eq!(page_count(&l), 1);
        assert_eq!(paginate(&t, &l), vec![vec!["hello", "world"]]);
    }

    #[test]
    fn an_empty_document_is_still_one_blank_page() {
        let (t, l) = wrapped("");
        assert_eq!(page_count(&l), 1);
        assert_eq!(paginate(&t, &l).len(), 1);
    }

    #[test]
    fn pages_break_where_the_status_line_says_they_do() {
        let text = (0..(LINES_PER_PAGE + 1))
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (t, l) = wrapped(&text);
        let pages = paginate(&t, &l);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), LINES_PER_PAGE);
        assert_eq!(pages[1], vec![format!("line {LINES_PER_PAGE}")]);
        assert_eq!(page_of(LINES_PER_PAGE - 1), 0);
        assert_eq!(page_of(LINES_PER_PAGE), 1);
    }

    #[test]
    fn rendering_adds_the_margins_and_a_form_feed_between_pages() {
        let pages = vec![vec!["one".to_string()], vec!["two".to_string()]];
        let out = render(&pages);
        let expected = format!(
            "{top}{pad}one\n\x0c{top}{pad}two\n",
            top = "\n".repeat(TOP_MARGIN),
            pad = " ".repeat(LEFT_MARGIN)
        );
        assert_eq!(out, expected);
        assert!(!out.ends_with('\x0c'), "no trailing form feed");
    }

    #[test]
    fn blank_lines_carry_no_margin_spaces() {
        let out = render(&[vec![String::new(), "x".to_string()]]);
        let body: Vec<&str> = out.lines().skip(TOP_MARGIN).collect();
        assert_eq!(body, vec!["", "          x"]);
    }

    #[test]
    fn a_single_page_job_takes_just_that_page() {
        let text = (0..(LINES_PER_PAGE + 1))
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (t, l) = wrapped(&text);
        let second = job(&t, &l, Scope::Page(1)).expect("page 2 exists");
        assert!(second.contains(&format!("line {LINES_PER_PAGE}")));
        assert!(!second.contains("line 0\n"));
        assert!(!second.contains('\x0c'));
        assert_eq!(job(&t, &l, Scope::Page(5)), None, "no such page");
    }
}
