# Screenplay mode

**Date:** 2026-10-28
**Status:** Approved, building

## 1. Overview

A second way of writing in Lone Typist: a screenplay, laid out on screen
at its true page geometry and printed as a manuscript that looks like it
came out of a typewriter.

Screenplay format was invented on the typewriter. Courier at ten
characters to the inch and six lines to the inch is the format's ruler,
and that is the same grid the app already runs on: `status.rs` measures
pages at 10 cpi / 6 lpi with 54 text lines a page. So the screen does not
approximate the page. Every element sits on screen exactly where it will
sit on paper, one cell to one typed character.

### Goals

- Write a screenplay without thinking about format. Enter and Tab move
  between elements; indents and capitals happen on their own.
- What you see is the page: the same indents, the same line breaks, and
  the same page breaks the printout will have.
- Printing produces a typewritten manuscript: Courier 10 Pitch, uneven
  strikes, ink that soaks into the paper. A clean setting exists for
  contest and agent submissions.
- Files stay plain text in an open format that other screenwriting
  programs read.

### Non-goals

Dual dialogue, revision marks and coloured pages, scene numbers,
automatic `(CONT'D)` for a speaker returning after action, Final Draft
`.fdx` export. All are additive and can arrive later.

## 2. Decisions

| # | Decision | Rationale |
|---|---|---|
| S1 | Files are [Fountain](https://fountain.io) | Plain text, open spec, read by Highland, Fade In, Beat, WriterDuet. Keeps design decision D3. |
| S2 | The buffer holds unindented Fountain; indents are layout | The file stays valid Fountain; editing, undo and selection are unchanged. |
| S3 | Final Draft's Enter/Tab model | It is what screenwriters' hands already know. |
| S4 | The line the caret is on counts as content | Lets "NAME + empty line below" be a character cue while you are about to type the dialogue, without forcing markers in the file. |
| S5 | One paginator for screen and paper | Page numbers on the status line are the page numbers on paper. |
| S6 | Print through our own PDF, sent to `lpr` | CUPS's text filter sets clean digital Courier; a typewriter look needs per-character control. |
| S7 | Courier 10 Pitch BT, bundled | IBM's typewriter Courier. Bitstream's licence permits use, modification and redistribution with the notice kept intact (`assets/fonts/COURIER10-COPYING`). |
| S8 | Typewriter variance is procedural and deterministic | A pre-distressed font repeats the same flaw on every "e". Seeded by position, so a reprint matches. |

## 3. Turning it on

- A `.fountain` file opens in screenplay mode.
- File > New Screenplay (Cmd-Shift-N) starts one, seeded with `FADE IN:`.
- View > Mode: Screenplay (F8) switches the open document either way.
- Saving an unnamed screenplay suggests `untitled.fountain`.

## 4. Elements and geometry

Paper columns count from the left edge at 10 cpi. The screen shows paper
column *c* at screen column *c* − 5, which centres the 6in text block in
the 80-column grid.

| Element | Paper col | Width | Notes |
|---|---|---|---|
| Scene heading | 15 | 60 | `INT.`, `EXT.`, `EST.`, `INT./EXT.`, `I/E`; or forced with `.` |
| Action | 15 | 60 | Forced with `!` |
| Character | 37 | 38 | All caps after a blank line, dialogue below. Forced with `@` |
| Parenthetical | 31 | 24 | `(` inside a dialogue block |
| Dialogue | 25 | 35 | |
| Transition | right edge at 75 | 60 | All caps ending `TO:`; or forced with `>` |
| Centred | centred in 15..75 | 60 | `>THE END<` |
| Page number | right edge at 75, line 3 | | `12.`; none on page 1 |

Notes `[[ ]]`, boneyard `/* */`, sections `#` and synopses `=` show dim
on screen and never print. Scene headings show bright.

Margins: 1.5in left, 1in right, top and bottom; 54 body lines a page.

## 5. Typing

**Enter** ends the element at the caret and starts the right next one:

| On | Enter gives |
|---|---|
| Scene heading | blank line, then action |
| Action | blank line, then action. An all-caps line after a blank becomes a character cue instead |
| Character | dialogue on the next line |
| Parenthetical | dialogue on the next line |
| Dialogue | blank line, then action |
| Transition | blank line, then action |

Enter in the middle of a line splits it, as in prose.

**Tab**
- On an empty line: cycles the line through Character, Transition, Scene
  heading, and back to Action. The line moves to that indent and typing
  capitalises.
- On dialogue: opens a parenthetical, `()` with the caret inside.
- When a completion is offered: accepts it.

**Capitals.** Scene headings, character cues and transitions capitalise
as you type; a heading typed as `int.` is raised to `INT.` the moment it
becomes one.

**Completion.** Character cues complete from names already in the script,
most-used first. Scene headings complete locations after `INT. `/`EXT. `
and times after ` - `. The suggestion shows in dim text after the caret;
Tab or Right accepts, and Enter accepts a character name.

## 6. Pagination

The paginator lays out the whole script into pages of 54 lines:

- Blank lines at the top of a page are dropped.
- A scene heading needs its blank line and two lines of what follows on
  the same page, or it moves to the next.
- Action splits only between lines, keeping two on each side, preferring
  a line that ends a sentence. A block longer than a page splits where
  it must.
- Dialogue splits only after a line of dialogue (never after the cue or
  a parenthetical), with `(MORE)` under it at the cue's indent and
  `NAME (CONT'D)` heading the next page. Otherwise the block moves whole.
- `===` forces a page break.

On screen the paginator's output becomes the rows you scroll through: a
rule marks each page break with the page number, and `(MORE)` /
`(CONT'D)` appear where they will print. These rows are not text; the
caret steps over them.

## 7. The title page

Tools > Title Page edits Title, Credit, Author, Source, Draft date and
Contact, stored as Fountain's key/value block at the top of the file.
It prints as an unnumbered first sheet: title in caps, underlined, a
third of the way down; credit and author centred below; contact bottom
left, draft date bottom right.

## 8. The status line and navigation

- Right field: `Dialogue   Sc 12   Pg 34 of 97`.
- F6 shows `97 pages  112 scenes  ~97 min`.
- Tools > Go to Scene (Cmd-G) lists every heading with its page; Enter
  jumps.

## 9. Printing

Print (Shift-F7) keeps its screen. In screenplay mode it gains
`R - Ribbon` cycling Fresh / Worn / Clean (saved in the config), and
Document to Disk writes a `.pdf`.

The PDF is US Letter, Courier 10 Pitch BT at 12pt, every character
placed on the 10 cpi / 6 lpi grid by hand:

- **Strike position**: up to ±0.25pt across and ±0.35pt down per
  character, plus a slight per-line drift, as type bars and a platen
  allow.
- **Strike density**: ink opacity varies per character. Fresh ribbon
  0.82-1.0, worn 0.55-0.9 with occasional starved strikes.
- **Ink spread**: glyphs are filled and lightly stroked, so ink appears
  to soak into the paper. Now and then a faint second impression sits a
  fraction of a point off.
- **Emphasis** is done the way a typewriter did it: `_underline_` and
  `*italic*` overstrike an underscore; `**bold**` strikes twice.
- **Clean** turns all of it off: plain black Courier 10 Pitch on the
  grid.

Variance is seeded from page, line and column, so the same script
prints the same way twice. The PDF goes to `lpr` as is; CUPS takes PDF
directly.

## 10. Code layout

- `fountain.rs`: classifies each source line into an element. Pure.
- `script.rs`: element geometry, wrapping, pagination, screen rows,
  scenes, character names, title page. Pure.
- `typewriter.rs`: pages to PDF bytes.
- `app.rs`: the mode, Enter/Tab/caps/completion, rows in the viewport,
  status, the scene list and title page overlays.
- `main.rs`: `.fountain` detection, save naming, PDF printing, ribbon
  setting.
