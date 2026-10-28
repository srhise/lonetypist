# Lone Typist

A distraction-free writing app that emulates VGA text mode.

The nostalgia here is architectural, not cosmetic. The app maintains a
real 80×25 grid of character cells, blits them from the IBM VGA ROM font
into a 720×400 framebuffer, and passes that through a CRT shader. It is a
DOS screen because it is built like one.

The name is for whoever is still up past midnight with one lamp on,
typing the report nobody else is going to believe.

Inspired by WordPerfect 6.0 for DOS.

```
   It was a bright cold day in April, and the clocks were striking
   thirteen. Winston Smith, his chin nuzzled into his breast in an
   effort to escape the vile wind, slipped quickly through the glass
   doors of Victory Mansions.█



C:\USERS\SRHISE\DOCUMENTS\CHAPTER-ONE.TXT *   Doc 1   Pg 1   Ln 2"   Pos 6.3"
```

## Install

macOS 11+, Apple Silicon and Intel. Download the DMG from the
[latest release](https://github.com/srhise/lonetypist/releases/latest),
or:

```sh
brew install --cask srhise/tap/lone-typist
```

## Build

Needs a Rust toolchain. Nothing else.

```sh
cargo run                 # run it
cargo test                # 352 tests, all headless
./tools/package.sh        # build "target/Lone Typist.app"
./tools/release.sh        # sign, notarize, draft a GitHub release
./tools/asc-bootstrap.sh  # certificates and profile, via the API
./tools/appstore.sh       # sandboxed build, validated for the App Store
```

`vendor/winit-0.30.13` is winit with one function emptied out. It called
the private `CGSSetWindowBackgroundBlurRadius` to blur a window
background — something this app never asks for — and the Mac App Store
rejects any binary that so much as references it. winit fixes this in
0.31 by moving to the public `NSVisualEffectView`; when that leaves
beta, delete `vendor/` and the `[patch.crates-io]` block. Before any
submission, check the binary itself:

```sh
nm -u target/release/lonetypist | grep CGSSet   # must print nothing
```

The two App Store scripts need an App Store Connect API key: its id in
`ASC_KEY_ID`, the team's issuer id in `ASC_ISSUER_ID`, and
`AuthKey_<id>.p8` in `~/.appstoreconnect/private_keys/`. `tools/asc.rb`
talks to the API directly (`tools/asc.rb apps`, `certs`, `profiles`) and
needs nothing installed — Ruby's standard library can sign the token.

## Menu

**`F1` or `Esc`** drops the menu bar (`Alt-=` too, WordPerfect's own).
Esc has nothing else to do while you are writing, and unlike an F-key it
arrives whatever your keyboard is set to. `←→` walk the bar, `↑↓` the
items, a letter jumps straight to one, `Enter` fires, `Esc` backs out a
level at a time. Every item shows its hotkey on the right, so the menu
teaches the shortcuts and then you stop needing it.

```
 File  Edit  View  Tools  Help
┌────────────────────────────┐
│ Retrieve...       Shft-F10 │
│ Open (browse)...     Cmd-O │
│ Save                 Cmd-S │
│ Save As...             F10 │
│ New                  Cmd-N │
│ Draft Screenplay Cmd-Shft-N│
├────────────────────────────┤
│ Print...           Shft-F7 │
├────────────────────────────┤
│ Exit                    F7 │
└────────────────────────────┘
```

## Naming

It opens on a blank, untitled draft with the File menu pulled down over
it, so New, Open and Draft Screenplay are the first thing you see. It
asks nothing of you: start typing and the menu gets out of the way and
the words land on the page; one `Esc` puts it away; `Enter` or the
arrows use it. The document reaches disk the first time you save it.

Saving something new -- `Cmd-S` on an untitled draft, or `F10` / `Cmd-Shift-S`
(Save As) any time -- opens the macOS save panel beside the current
file, or in your writing folder, with the current name filled in.
Naming a draft `.fountain` makes it a screenplay.

`Shift-F10` (Retrieve) is the in-world way in: type a name and a bare
`chapter-one.txt` comes from the base directory (`~/Documents` unless
`base_dir` says otherwise in `config.toml`), while anything containing
`/` or `~` is read as a path, the way a shell would. `Cmd-O` is the
native panel, for browsing.

## Printing

`Shift-F7` (or `Cmd-P`) takes over the screen the way WordPerfect's
print menu did:

```
Print

     1 - Full Document
     2 - Page
     3 - Document to Disk

Options

     Printer               HP_LaserJet
     Pages                 3   (cursor on page 2)



Selection: 0
```

The job is plain text at 10 characters and 6 lines to the inch with a
one-inch margin all round and a form feed between pages -- the same
geometry the status line reports, so `Pg 3` on screen is page 3 on
paper. It goes to the system default printer through `lpr`. `3` writes
exactly what the printer would have received to a `.prn` file instead.

## Screenplays

`Cmd-Shift-N` (File > Draft Screenplay) starts one; any `.fountain`
file opens as one; `F8` switches the open document either way. The file
is [Fountain](https://fountain.io), plain text that Highland, Fade In,
Beat and WriterDuet all read, so nothing is locked in here either.

Screenplay format was made on the typewriter, at the same 10
characters and 6 lines to the inch this screen already runs on. So the
screen does not approximate the page: paper column *c* is screen
column *c* − 5, and every element sits where it will be typed.

```
          INT. FBI BASEMENT OFFICE - NIGHT

          A single desk lamp. Files stacked to the ceiling.

                                REYES
                          (not looking up)
                    You're late. The tape's already
                    rolling.

                                                      CUT TO:
```

| On | Enter gives | Tab gives |
|---|---|---|
| an empty line | a new line | cue, then transition, then heading |
| a scene heading | a blank line, then action | |
| action | a blank line, then action; capitals become a cue | |
| a cue | its dialogue | `(` a parenthetical `)` |
| dialogue | a blank line, then action | `(` a parenthetical `)` |

Headings, cues and transitions capitalise as you type. Cues complete
from names already in the script, headings from its places and times;
the suggestion is dim after the caret, and `Tab` or `→` takes it.
`Cmd-G` lists every scene with its page. Tools > Title Page fills in the
title page.

Pages break the way a typist broke them: no heading stranded at the
foot of a page, action split only at a sentence with two lines either
side, a speech split only between sentences with `(MORE)` under it and
`NAME (CONT'D)` over the rest. The screen draws each break as a rule
with the page number, and shows the `(MORE)` where it will print. The
status line reads `Dialogue   Sc 12   Pg 34/97`.

A screenplay prints as a PDF typed in Courier 10 Pitch, IBM's
typewriter face. No two strikes are alike: each lands a hair off the
grid, the ink density wanders, a fabric ribbon leaves its weave in the
letters, and every key has its own habits, so every `e` on the page
leans the same way. Italic and underline overstrike an underscore and
bold strikes twice, as a typewriter had to. `R` on the print screen
picks the ribbon: **Fresh**, **Worn**, or **Clean**, plain black type
for contest and agency submissions. The variance is seeded by
position, so a script prints the same way twice.

```sh
cargo test dump_sample_pdf -- --ignored   # target/screenplay-*.pdf
```

## Keys

DOS look, modern muscle memory.

| | |
|---|---|
| `F1` or `Esc` (or `Alt-=`) | Menu bar |
| `Shift-F1` | Help |
| `Cmd-N` / `Cmd-O` / `Cmd-S` | New, browse-open, save |
| `F10` / `Cmd-Shift-S` | Save As (the save panel) |
| `Shift-F10` | Retrieve (typed name) |
| `F7` | Exit |
| `Shift-F7` / `Cmd-P` | Print |
| `Cmd-Z` / `Cmd-Shift-Z` | Undo, redo — one word at a time |
| `Cmd-A` / `Cmd-C` / `Cmd-X` / `Cmd-V` | Select all, copy, cut, paste |
| `Opt-Arrow` | Move by word |
| `Cmd-Arrow` | Line start/end, document start/end |
| `Cmd-Q` | Quit |
| `F3` | CRT effects on/off |
| `F5` | 80×25 / 80×50 |
| `F6` | Word count |
| `F8` | Screenplay / prose |
| `F9` | Typewriter sounds on/off |
| `F11` | Fullscreen |
| `Esc` | Close any box |

**Typewriter sounds** (`F9`, View > Typewriter Sounds) are off until
you ask. They are a Hermes Precisa 305: eight different key strikes,
played a little faster or slower and louder or softer each time so no
two in a row match (the space bar included), the carriage return on
`Enter`, and the bell seven characters short of the margin. The audio device is only
opened while they are on.

The mouse places the caret, drags to select, and scrolls. There is no
permanent F-key hint bar: it would be a second row of chrome you read
once and then never again. `Shift-F1` and the menu both cover it.

## The cursor

A thin underline on the bottom scanlines of the cell — the VGA hardware
cursor, not a character drawn over your text. It holds perfectly steady
while you type and for a second after you stop, then blinks at about
1.2Hz. Nothing on screen moves while you are writing.

## Files

Plain UTF-8 `.txt`, anywhere on disk. No proprietary format, no library
folder, nothing to export from. Saves are atomic — a failed write never
destroys the previous version.

Text wraps at 65 columns, centred in the 80-column screen. That is a 6.5″
line at 10 characters per inch: an 8.5″ page with 1″ margins, which is
exactly what the `Pos` readout in the status line is measuring.

### Two things it does to your files

Both are consequences of the character grid being real rather than themed,
and both are worth knowing before you trust it with a manuscript:

- **Tabs are expanded to spaces** (8-column stops) when a file is opened,
  and do not survive a round trip. Expanding them at the boundary is what
  lets every column calculation downstream be exact.
- **Only CP437 characters can be typed.** That is ASCII plus accented
  Latin, Greek, and box drawing — the 256 glyphs in the VGA ROM. No CJK,
  no Cyrillic, no emoji: there is no glyph to draw them with, so they are
  rejected at the input boundary rather than stored and shown as blanks.
  Curly quotes, en and em dashes, and ellipses are silently converted to
  their ASCII equivalents on the way in.

Line endings are the exception: CRLF files stay CRLF files.

## Backups

Every 30 seconds a modified document is copied to:

```
~/Library/Application Support/lonetypist/backup/
```

This never overwrites your own file — it mirrors WordPerfect's timed
backup. If a backup outlives its document, the next launch offers to
recover it. Settings live beside it in `config.toml`; delete it to reset.

## Two builds, one binary

The download here and the Mac App Store copy are the same program. The
App Store requires the sandbox, which rewrites `$HOME` to a private
container: a typed path like `~/Documents/chapter-one.txt` would save
somewhere you cannot see, and report success. So the app asks the OS
whether it is sandboxed (`APP_SANDBOX_CONTAINER_ID`) and, if it is:

- asks once for a writing folder, and keeps a security-scoped bookmark
  to it so the permission survives a relaunch,
- resolves bare names inside that folder, and refuses a typed path that
  leaves it rather than writing somewhere invisible. `Cmd-O` still
  reaches anywhere, because the Open panel grants access as you pick.

Outside the sandbox none of that applies and the path rules are the
shell's. The fence itself is `src/paths.rs`, which is pure and tested;
`src/scope.rs` holds the small amount of Objective-C that bookmarks
need.

## How it fits together

Four layers, each depending only on the one below. The top three are pure
functions over data with no windowing dependency, which is why the whole
application logic tests headless.

```
 keystrokes → editor → vga grid → framebuffer → crt.wgsl → window
              ‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾
                    pure, fully tested
```

| | |
|---|---|
| `src/editor.rs` | Text buffer, cursor, selection, undo |
| `src/wrap.rs` | Soft wrap; offset ↔ (line, column) |
| `src/fountain.rs` | Which screenplay element each line is |
| `src/script.rs` | Screenplay geometry, pagination, screen rows |
| `src/typewriter.rs` | The typewritten PDF |
| `src/sound.rs` | Typewriter sounds and their mixer |
| `src/vga.rs` | Cell grid, palette, framebuffer rasterizer |
| `src/font.rs` | The embedded ROM fonts |
| `src/cp437.rs` | Encoding and the input filter |
| `src/status.rs` | Page/line/position arithmetic |
| `src/fileio.rs` | Load, atomic save, normalization |
| `src/backup.rs` | Timed backups and recovery |
| `src/menu.rs` | The menu tree and its navigation |
| `src/input.rs` | The single-line field in the modals |
| `src/app.rs` | State, focus routing, the command interpreter |
| `src/main.rs` | Window, events, everything OS-facing |

Two ignored tests dump what the renderer actually produces, which is the
fastest way to see a change:

```sh
cargo test dump_app_preview -- --ignored   # target/app-preview.bmp
```

Design notes are in `docs/superpowers/`.

## Fonts

`assets/fonts/*.bin` are raw character-generator ROM dumps from IBM VGA
hardware. `Courier10PitchBT-Roman.otf` is Bitstream's Courier 10 Pitch,
which screenplays print in; Bitstream's notice is in
`assets/fonts/COURIER10-COPYING` and ships inside the app. See
`assets/fonts/PROVENANCE.md`.

## Sounds

`assets/sounds/*.wav` are cut from Joseph Sardin's CC0 recordings of a
Hermes Precisa 305 on BigSoundBank. See `assets/sounds/PROVENANCE.md`.

## License

MIT. See `LICENSE`.
