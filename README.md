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
cargo test                # 272 tests, all headless
./tools/package.sh        # build "target/Lone Typist.app"
./tools/release.sh        # sign, notarize, draft a GitHub release
./tools/asc-bootstrap.sh  # certificates and profile, via the API
./tools/appstore.sh       # sandboxed build, validated for the App Store
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
├────────────────────────────┤
│ Print...           Shft-F7 │
├────────────────────────────┤
│ Exit                    F7 │
└────────────────────────────┘
```

## Naming

It opens by asking what you are about to write. `Enter` names the
document, `Esc` skips straight to an untitled buffer — the prompt never
stands between you and a sudden idea.

A bare `chapter-one.txt` lands in the base directory (`~/Documents`
unless `base_dir` says otherwise in `config.toml`); anything containing
`/` or `~` is read as a path, the way a shell would. Naming at launch
only sets the destination — the file appears on the first save.

`F10` (Save As) and `Shift-F10` (Retrieve) use the same in-world box.
`Cmd-O` keeps the native macOS panel, because typing a path is a poor
way to *browse*.

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

## Keys

DOS look, modern muscle memory.

| | |
|---|---|
| `F1` or `Esc` (or `Alt-=`) | Menu bar |
| `Shift-F1` | Help |
| `Cmd-N` / `Cmd-O` / `Cmd-S` | New, browse-open, save |
| `F10` / `Shift-F10` | Save As, Retrieve (typed name) |
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
| `F11` | Fullscreen |
| `Esc` | Close any box |

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
hardware. See `assets/fonts/PROVENANCE.md`.

## License

MIT. See `LICENSE`.
