//! WordPerfect's status arithmetic, in inches, from the printer metrics
//! of the era: 6 lines per inch, 10 characters per inch, 1in margins,
//! and 54 text lines on a page (a 9in text block at 6 lpi).

use std::path::Path;

const LINES_PER_INCH: f32 = 6.0;
const CHARS_PER_INCH: f32 = 10.0;
pub const LINES_PER_PAGE: usize = 54;
const MARGIN_INCHES: f32 = 1.0;

/// How much of the status line the path may occupy before truncation.
pub const MAX_PATH_CELLS: usize = 44;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Position {
    pub page: usize,
    pub line_inches: f32,
    pub pos_inches: f32,
}

pub fn measure(visual_line: usize, col: usize) -> Position {
    Position {
        page: visual_line / LINES_PER_PAGE + 1,
        line_inches: MARGIN_INCHES + (visual_line % LINES_PER_PAGE) as f32 / LINES_PER_INCH,
        pos_inches: MARGIN_INCHES + col as f32 / CHARS_PER_INCH,
    }
}

/// Two decimal places, trailing zeros stripped, inch mark appended.
pub fn format_inches(v: f32) -> String {
    let s = format!("{v:.2}");
    let trimmed = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    format!("{trimmed}\"")
}

pub fn right_field(p: &Position) -> String {
    format!(
        "Doc 1   Pg {}   Ln {}   Pos {}",
        p.page,
        format_inches(p.line_inches),
        format_inches(p.pos_inches)
    )
}

/// A POSIX path dressed as a DOS one, because that is what the era looked
/// like. Truncates from the left so the filename always survives.
pub fn dos_path(path: Option<&Path>, dirty: bool) -> String {
    let flag = if dirty { " *" } else { "" };
    let Some(path) = path else {
        return format!("(UNTITLED){flag}");
    };

    let text = path.to_string_lossy().to_uppercase().replace('/', "\\");
    let text = format!("C:{text}");
    let budget = MAX_PATH_CELLS.saturating_sub(flag.len());

    let shown = if text.chars().count() > budget {
        let keep = budget.saturating_sub(3);
        let start = text.chars().count() - keep;
        let tail: String = text.chars().skip(start).collect();
        format!("...{tail}")
    } else {
        text
    };
    format!("{shown}{flag}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_origin_is_one_inch_on_both_axes() {
        let p = measure(0, 0);
        assert_eq!(p.page, 1);
        assert!((p.line_inches - 1.0).abs() < 1e-6);
        assert!((p.pos_inches - 1.0).abs() < 1e-6);
    }

    #[test]
    fn lines_advance_by_one_sixth_of_an_inch() {
        assert!((measure(1, 0).line_inches - (1.0 + 1.0 / 6.0)).abs() < 1e-6);
        assert!((measure(6, 0).line_inches - 2.0).abs() < 1e-6);
    }

    #[test]
    fn columns_advance_by_one_tenth_of_an_inch() {
        assert!((measure(0, 1).pos_inches - 1.1).abs() < 1e-6);
        assert!((measure(0, 10).pos_inches - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_page_is_fifty_four_lines() {
        assert_eq!(measure(53, 0).page, 1, "last line of page 1");
        assert_eq!(measure(54, 0).page, 2, "first line of page 2");
        assert_eq!(measure(107, 0).page, 2);
        assert_eq!(measure(108, 0).page, 3);
    }

    #[test]
    fn the_line_measurement_resets_on_each_page() {
        assert!((measure(54, 0).line_inches - 1.0).abs() < 1e-6);
        assert!((measure(55, 0).line_inches - (1.0 + 1.0 / 6.0)).abs() < 1e-6);
    }

    #[test]
    fn whole_inches_print_without_a_decimal_point() {
        assert_eq!(format_inches(1.0), "1\"");
        assert_eq!(format_inches(2.0), "2\"");
    }

    #[test]
    fn fractional_inches_print_to_two_places_with_zeros_stripped() {
        assert_eq!(format_inches(1.0 + 1.0 / 6.0), "1.17\"");
        assert_eq!(format_inches(2.5), "2.5\"");
        assert_eq!(format_inches(1.1), "1.1\"");
        assert_eq!(format_inches(1.25), "1.25\"");
    }

    #[test]
    fn the_right_field_reads_like_wordperfect() {
        assert_eq!(
            right_field(&measure(0, 0)),
            "Doc 1   Pg 1   Ln 1\"   Pos 1\""
        );
        assert_eq!(
            right_field(&measure(1, 5)),
            "Doc 1   Pg 1   Ln 1.17\"   Pos 1.5\""
        );
    }

    #[test]
    fn a_path_renders_as_an_uppercase_dos_path() {
        let p = Path::new("/Users/srhise/Documents/ch1.txt");
        assert_eq!(
            dos_path(Some(p), false),
            "C:\\USERS\\SRHISE\\DOCUMENTS\\CH1.TXT"
        );
    }

    #[test]
    fn a_modified_document_is_flagged() {
        let p = Path::new("/tmp/a.txt");
        assert_eq!(dos_path(Some(p), true), "C:\\TMP\\A.TXT *");
    }

    #[test]
    fn an_unsaved_document_has_no_path() {
        assert_eq!(dos_path(None, false), "(UNTITLED)");
        assert_eq!(dos_path(None, true), "(UNTITLED) *");
    }

    #[test]
    fn a_long_path_is_truncated_from_the_left() {
        let p = Path::new("/a/very/deeply/nested/directory/structure/that/goes/on/file.txt");
        let out = dos_path(Some(p), false);
        assert!(
            out.len() <= MAX_PATH_CELLS,
            "got {} cells: {out}",
            out.len()
        );
        assert!(out.starts_with("..."), "truncation is marked: {out}");
        assert!(
            out.ends_with("FILE.TXT"),
            "the filename always survives: {out}"
        );
    }
}
