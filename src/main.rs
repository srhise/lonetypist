mod app;
mod backup;
mod config;
mod cp437;
mod editor;
mod fileio;
mod font;
mod input;
mod keymap;
mod menu;
mod overlay;
mod present;
mod print;
#[cfg(test)]
mod simulate;
mod status;
#[cfg(test)]
mod stress;
mod vga;
mod wrap;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{Modifiers, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

use input::Purpose;
use keymap::Command;
use overlay::{Overlay, Prompt};
use print::{Request, Scope};
use vga::{FB_HEIGHT, FB_WIDTH};

/// VGA text mode was shown on a 4:3 monitor; the same letterboxing the
/// shader applies has to be undone to map clicks back to cells.
const DISPLAY_ASPECT: f64 = 4.0 / 3.0;

/// How often an idle window wakes. The cursor's own blink policy lives
/// in `app`; this only has to be finer than it so the phase stays true.
const TICK_MS: u64 = 200;

/// The window and everything the OS owns. All document state lives in
/// `app::App`; this shell only translates events and draws.
struct Shell {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    present: Option<present::Present>,
    state: app::App,
    modifiers: Modifiers,
    clipboard: Option<arboard::Clipboard>,
    mouse_cell: (usize, usize),
    mouse_down: bool,
    start: Instant,
    config: config::Config,
    /// Text recovered from a backup, held until the prompt is answered.
    recovery: Option<String>,
    last_backup_ms: u64,
    last_fullscreen: bool,
}

impl Shell {
    fn new() -> Self {
        Self {
            window: None,
            pixels: None,
            present: None,
            state: app::App::new(),
            modifiers: Modifiers::default(),
            clipboard: None,
            mouse_cell: (0, 0),
            mouse_down: false,
            start: Instant::now(),
            config: config::Config::default(),
            recovery: None,
            last_backup_ms: 0,
            last_fullscreen: false,
        }
    }

    /// Push toggles the user flipped out to the window and the config
    /// file. Cheap enough to call after every command.
    fn sync_settings(&mut self) {
        let effects = self.state.effects();
        let dense = self.state.dense();
        let fullscreen = self.state.fullscreen();

        if fullscreen != self.last_fullscreen {
            if let Some(w) = self.window.as_ref() {
                w.set_fullscreen(if fullscreen {
                    Some(Fullscreen::Borderless(None))
                } else {
                    None
                });
            }
            self.last_fullscreen = fullscreen;
        }

        if effects != self.config.effects
            || dense != self.config.dense
            || fullscreen != self.config.fullscreen
        {
            self.config.effects = effects;
            self.config.dense = dense;
            self.config.fullscreen = fullscreen;
            config::save(&self.config);
        }
    }

    /// Write a periodic backup if the document has unsaved changes.
    fn maybe_backup(&mut self) {
        let now = self.now_ms();
        if !self.state.editor().is_dirty() {
            return;
        }
        if now.saturating_sub(self.last_backup_ms) < backup::INTERVAL_MS {
            return;
        }
        self.last_backup_ms = now;
        let text = self.state.editor().to_string();
        let _ = backup::write(self.state.path(), &text);
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Physical window position -> grid cell, undoing the letterbox.
    fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let window = self.window.as_ref()?;
        let size = window.inner_size();
        let (w, h) = (size.width.max(1) as f64, size.height.max(1) as f64);

        let (draw_w, draw_h) = if w / h > DISPLAY_ASPECT {
            (h * DISPLAY_ASPECT, h)
        } else {
            (w, w / DISPLAY_ASPECT)
        };
        let ox = (w - draw_w) / 2.0;
        let oy = (h - draw_h) / 2.0;

        let u = (x - ox) / draw_w;
        let v = (y - oy) / draw_h;
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            return None;
        }

        let rows = self.state.screen().rows();
        Some((
            ((u * 80.0) as usize).min(79),
            ((v * rows as f64) as usize).min(rows - 1),
        ))
    }

    fn do_open(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text", &["txt", "md", "text"])
            .pick_file()
        else {
            return;
        };
        match fileio::load(&path) {
            Ok(loaded) => {
                self.state.load_text(&loaded.text);
                self.state.set_path(path, loaded.crlf);
            }
            Err(e) => self.state.set_overlay(Overlay::Message {
                title: "Error".to_string(),
                body: format!("Cannot open file: {e}"),
                danger: true,
            }),
        }
    }

    /// Returns whether the document ended up saved.
    fn do_save(&mut self, force_dialog: bool) -> bool {
        let path = match (self.state.path(), force_dialog) {
            (Some(p), false) => p.to_path_buf(),
            _ => match rfd::FileDialog::new()
                .set_file_name("untitled.txt")
                .save_file()
            {
                Some(p) => p,
                None => return false,
            },
        };
        let text = self.state.editor().to_string();
        let crlf = self.state.crlf();
        match fileio::save(&path, &text, crlf) {
            Ok(()) => {
                backup::clear(self.state.path());
                self.state.set_path(path, crlf);
                self.state.mark_saved();
                true
            }
            Err(e) => {
                self.state.set_overlay(Overlay::Message {
                    title: "Error".to_string(),
                    body: format!("Cannot save: {e}"),
                    danger: true,
                });
                false
            }
        }
    }

    /// A bare name lands in the base directory; anything with a slash or
    /// a tilde is taken as a path, the way a shell would read it.
    fn resolve_name(&self, value: &str) -> std::path::PathBuf {
        let value = value.trim();
        if let Some(rest) = value.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(rest);
            }
        }
        let p = std::path::Path::new(value);
        if p.is_absolute() || value.contains('/') {
            return p.to_path_buf();
        }
        self.base_dir().join(value)
    }

    fn base_dir(&self) -> std::path::PathBuf {
        self.config
            .base_dir
            .clone()
            .or_else(dirs::document_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    fn act_on_field(&mut self, purpose: Purpose, value: &str) {
        let path = self.resolve_name(value);
        match purpose {
            Purpose::Retrieve => match fileio::load(&path) {
                Ok(loaded) => {
                    self.state.load_text(&loaded.text);
                    self.state.set_path(path, loaded.crlf);
                }
                Err(e) => self.state.set_overlay(Overlay::Message {
                    title: "Error".to_string(),
                    body: format!("Cannot retrieve: {e}"),
                    danger: true,
                }),
            },
            Purpose::PrintToFile => match self.state.print_job(Scope::Full) {
                Some(job) => match std::fs::write(&path, job) {
                    Ok(()) => {
                        let shown = status::dos_path(Some(&path), false);
                        self.state.notify(&format!("Printed to {shown}"));
                    }
                    Err(e) => self.error("Cannot print to disk", &e.to_string()),
                },
                None => self.error("Print", "Nothing to print."),
            },
            Purpose::SaveAs | Purpose::CreateAtLaunch => {
                // Naming at launch only sets the destination; the file
                // appears on the first save, as WordPerfect did.
                self.state.set_path(path, self.state.crlf());
                if purpose == Purpose::SaveAs {
                    self.do_save(false);
                }
            }
        }
    }

    fn do_new(&mut self) {
        self.state = app::App::new();
    }

    fn error(&mut self, title: &str, body: &str) {
        self.state.set_overlay(Overlay::Message {
            title: title.to_string(),
            body: body.to_string(),
            danger: true,
        });
    }

    /// The system's default printer, by asking CUPS the way the era
    /// would have looked at LPT1: `lpstat -d` prints
    /// "system default destination: NAME", or "no system default
    /// destination" when there is none.
    fn default_printer() -> Option<String> {
        let out = std::process::Command::new("lpstat")
            .arg("-d")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let name = text.trim().rsplit(':').next()?.trim();
        if name.is_empty() || text.contains("no system default") {
            return None;
        }
        Some(name.to_string())
    }

    fn do_print_screen(&mut self) {
        let printer = Self::default_printer();
        self.state.open_print(printer);
    }

    /// The name a printed-to-disk job gets: the document's name with a
    /// .PRN extension, or UNTITLED.PRN.
    fn prn_name(&self) -> String {
        self.state
            .path()
            .and_then(|p| p.file_stem())
            .map(|n| format!("{}.prn", n.to_string_lossy()))
            .unwrap_or_else(|| "untitled.prn".to_string())
    }

    /// Carry out what the print screen asked for. A printer job goes
    /// through `lpr` as plain text at 10 cpi and 6 lpi, which is what
    /// the page geometry assumes.
    fn do_print(&mut self, request: Request) {
        use std::io::Write;
        use std::process::{Command as Proc, Stdio};

        let scope = match request {
            Request::ToFile(_) => {
                let seed = self.prn_name();
                self.state
                    .open_field(Purpose::PrintToFile, "Document to Disk", "Filename:", &seed);
                return;
            }
            Request::Printer(scope) => scope,
        };
        let Some(job) = self.state.print_job(scope) else {
            self.error("Print", "Nothing to print.");
            return;
        };
        let pages = match scope {
            Scope::Full => self.state.page_count(),
            Scope::Page(_) => 1,
        };
        let title = self.prn_name().trim_end_matches(".prn").to_string();

        let spawned = Proc::new("lpr")
            .args(["-o", "cpi=10", "-o", "lpi=6", "-T", &title])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                self.error("Cannot print", &format!("lpr: {e}"));
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(job.as_bytes());
        }
        match child.wait_with_output() {
            Ok(out) if out.status.success() => {
                let printer = Self::default_printer().unwrap_or_else(|| "printer".to_string());
                let plural = if pages == 1 { "page" } else { "pages" };
                self.state
                    .notify(&format!("Sent {pages} {plural} to {printer}"));
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let err = if err.is_empty() {
                    "lpr failed".to_string()
                } else {
                    err
                };
                self.error("Cannot print", &err);
            }
            Err(e) => self.error("Cannot print", &format!("lpr: {e}")),
        }
    }

    /// Y or N on an open confirmation. Returns whether it was consumed.
    fn answer_prompt(&mut self, yes: bool, event_loop: &ActiveEventLoop) -> bool {
        let Some((prompt, yes)) = self.state.answer(yes) else {
            return false;
        };
        match (prompt, yes) {
            // "Save changes?" -> yes means save first, and a cancelled or
            // failed save aborts the whole action rather than losing work.
            (Prompt::QuitUnsaved, true) => {
                if self.do_save(false) {
                    event_loop.exit();
                }
            }
            (Prompt::QuitUnsaved, false) => {
                backup::clear(self.state.path());
                event_loop.exit();
            }
            (Prompt::NewUnsaved, true) => {
                if self.do_save(false) {
                    self.do_new();
                }
            }
            (Prompt::NewUnsaved, false) => self.do_new(),
            (Prompt::OpenUnsaved, true) => {
                if self.do_save(false) {
                    self.do_open();
                }
            }
            (Prompt::OpenUnsaved, false) => self.do_open(),
            (Prompt::Recover, true) => {
                if let Some(text) = self.recovery.take() {
                    self.state.load_text(&text);
                }
            }
            (Prompt::Recover, false) => {
                self.recovery = None;
                backup::clear(None);
            }
        }
        self.request_redraw();
        true
    }

    fn copy_to_clipboard(&mut self) {
        let text = self.state.editor().selected_text();
        if text.is_empty() {
            return;
        }
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(c) = self.clipboard.as_mut() {
            let _ = c.set_text(text);
        }
    }

    fn clipboard_text(&mut self) -> Option<String> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()?.get_text().ok()
    }

    /// Commands that need the OS are handled here; the rest go to state.
    fn dispatch(&mut self, cmd: Command, event_loop: &ActiveEventLoop) {
        let now = self.now_ms();
        let cmd = match cmd {
            Command::Copy => {
                self.copy_to_clipboard();
                return;
            }
            Command::Cut => {
                self.copy_to_clipboard();
                Command::Backspace
            }
            Command::Paste => match self.clipboard_text() {
                Some(t) if !t.is_empty() => Command::Insert(t),
                _ => return,
            },
            Command::Open => {
                if self.state.editor().is_dirty() {
                    self.state
                        .confirm(Prompt::OpenUnsaved, "Save changes to this document? (Y/N)");
                } else {
                    self.do_open();
                }
                self.request_redraw();
                return;
            }
            Command::New => {
                if self.state.editor().is_dirty() {
                    self.state
                        .confirm(Prompt::NewUnsaved, "Save changes to this document? (Y/N)");
                } else {
                    self.do_new();
                }
                self.request_redraw();
                return;
            }
            Command::Save => {
                self.do_save(false);
                self.request_redraw();
                return;
            }
            Command::SaveAs => {
                let seed = self
                    .state
                    .path()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.state
                    .open_field(Purpose::SaveAs, "Save Document", "Filename:", &seed);
                self.request_redraw();
                return;
            }
            Command::Retrieve => {
                self.state
                    .open_field(Purpose::Retrieve, "Retrieve Document", "Filename:", "");
                self.request_redraw();
                return;
            }
            Command::Print => {
                self.do_print_screen();
                self.request_redraw();
                return;
            }
            other => other,
        };

        self.state.apply(cmd, now);
        // A menu item may have fired one of the commands above, which
        // the app cannot perform itself.
        if let Some(fired) = self.state.take_deferred() {
            self.dispatch(fired, event_loop);
            return;
        }
        if let Some(request) = self.state.take_print_request() {
            self.do_print(request);
        }
        if let Some((purpose, value)) = self.state.take_submitted() {
            self.act_on_field(purpose, &value);
        }
        if self.state.should_quit {
            // A clean exit leaves no backup behind to recover from.
            backup::clear(self.state.path());
            event_loop.exit();
            return;
        }
        self.sync_settings();
        self.request_redraw();
    }

    fn redraw(&mut self) {
        let elapsed = self.now_ms();
        self.state.paint_at(elapsed);

        let (Some(pixels), Some(present), Some(window)) = (
            self.pixels.as_mut(),
            self.present.as_ref(),
            self.window.as_ref(),
        ) else {
            return;
        };
        self.state.screen().render(pixels.frame_mut());
        let size = window.inner_size();
        let params = present::Params {
            surface: (size.width, size.height),
            time: elapsed as f32 / 1000.0,
            effects: self.state.effects(),
            border: vga::PALETTE[app::BG as usize],
        };
        if let Err(e) = pixels.render_with(|encoder, target, context| {
            present.render(encoder, target, context, &params);
            Ok(())
        }) {
            eprintln!("render failed: {e}");
        }
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        self.config = config::load();
        self.state.set_effects(self.config.effects);
        self.state.set_dense(self.config.dense);

        let (cw, ch) = self.config.window;
        let attrs = Window::default_attributes()
            .with_title("Lone Typist")
            .with_inner_size(LogicalSize::new(cw.max(640) as f64, ch.max(480) as f64))
            .with_min_inner_size(LogicalSize::new(640.0, 480.0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("could not create a window: {e}");
                event_loop.exit();
                return;
            }
        };

        let size = window.inner_size();
        let surface = SurfaceTexture::new(size.width, size.height, Arc::clone(&window));
        let pixels = match Pixels::new(FB_WIDTH as u32, FB_HEIGHT as u32, surface) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("could not initialise the display: {e}");
                event_loop.exit();
                return;
            }
        };

        self.present = Some(present::Present::new(&pixels));
        self.pixels = Some(pixels);
        self.window = Some(window);

        if self.config.fullscreen {
            self.state.set_fullscreen(true);
            self.sync_settings();
        }

        // Ask what is being written before anything else -- Esc skips
        // straight to an untitled buffer.
        self.state.open_field(
            Purpose::CreateAtLaunch,
            "New Document",
            "Document to be created:",
            "",
        );

        // A backup that outlived its document means the last session did
        // not end cleanly.
        if let Some(b) = backup::pending(None) {
            if let Ok(loaded) = fileio::load(&b) {
                if !loaded.text.trim().is_empty() {
                    self.recovery = Some(loaded.text);
                    self.state
                        .confirm(Prompt::Recover, "A backup file exists.\nOpen it? (Y/N)");
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                config::save(&self.config);
                backup::clear(self.state.path());
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                if !self.state.fullscreen() {
                    self.config.window = (size.width, size.height);
                }
                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(e) = pixels.resize_surface(size.width.max(1), size.height.max(1)) {
                        eprintln!("resize failed: {e}");
                    }
                }
                self.request_redraw();
            }

            WindowEvent::ModifiersChanged(m) => self.modifiers = m,

            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return;
                }
                // A confirmation takes Y and N directly, the way the
                // era's prompts did.
                if matches!(self.state.overlay(), Overlay::Confirm { .. }) {
                    if let winit::keyboard::Key::Character(c) = &event.logical_key {
                        match c.to_lowercase().as_str() {
                            "y" => {
                                self.answer_prompt(true, event_loop);
                                return;
                            }
                            "n" => {
                                self.answer_prompt(false, event_loop);
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(cmd) = keymap::resolve(&event, &self.modifiers) {
                    self.dispatch(cmd, event_loop);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                if let Some(cell) = self.cell_at(position.x, position.y) {
                    self.mouse_cell = cell;
                    if self.mouse_down {
                        self.state.click(cell.0, cell.1, true);
                        self.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    self.mouse_down = state.is_pressed();
                    if self.mouse_down {
                        let (c, r) = self.mouse_cell;
                        self.state.click(c, r, false);
                        self.request_redraw();
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as i32,
                    MouseScrollDelta::PixelDelta(p) => -(p.y / 16.0) as i32,
                };
                if lines != 0 {
                    self.state.scroll(lines);
                    self.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Wake only often enough to blink the cursor: an idle window
        // costs essentially nothing.
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(TICK_MS),
        ));
        self.maybe_backup();
        self.request_redraw();
    }
}

/// Rust panics cannot unwind across the Objective-C event dispatch that
/// calls us, so they abort with no message anywhere. Record them first.
fn install_panic_logger() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A panic here aborts, and the abort panics again on the way out.
        // Only the first one explains anything, so never overwrite it.
        if let Some(dir) = config::app_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let log = dir.join("crash.log");
            if !log.exists() {
                let _ = std::fs::write(&log, format!("{info}\n"));
            }
        }
        eprintln!("lonetypist panicked: {info}");
        default(info);
    }));
}

fn main() {
    config::migrate_old_dir();
    install_panic_logger();

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("could not start: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell::new();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("exited with error: {e}");
    }
}
