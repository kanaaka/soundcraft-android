//! SoundCraft on Android (arm64-v8a): the real engine and egui shell, running on winit's
//! NativeActivity backend.
//!
//! # What this crate is
//!
//! A shell around the *unmodified* SoundCraft crates — `soundcraft-engine` (the session model, the
//! audio graph, the command registry) and `soundcraft-ui-egui` (the Edit and Mix windows, the
//! transport, the meters, the score editor and every dialog). Nothing in those crates is patched.
//! This crate supplies only what `apps/soundcraft` would normally supply: the [`Services`]
//! implementation, the preferences file, the autosave and preset folders, the window options and
//! the entry point. The same shape the six earlier ports in this suite used.
//!
//! # Why a separate crate, and not a feature on `apps/soundcraft`
//!
//! That crate cannot build for Android as written: it carries native dialogs (`rfd`), a CPAL audio
//! sink and a control server. `rfd` has **no Android backend at all** — its `backend.rs` compiles no
//! module for a target that is neither Linux/BSD, macOS, Windows nor wasm32 — so any call site stops
//! the build with `the trait bound FileDialog: FilePickerDialogImpl is not satisfied`.
//!
//! `soundcraft_ui_egui::Services` is a plain struct of three `Option<Box<dyn Fn…>>` fields that
//! derives `Default`, so the fix is not to patch the app but to build a platform that supplies what
//! it can and omits the rest. Its `ui-egui` is Android-clean: it has no eframe dependency and no
//! `rfd` dependency (the desktop shell owns both).
//!
//! # The file dialogs — this is the whole reason the shell is more than an entry point
//!
//! SoundCraft's dialogs ask for a path two ways. Almost every command with a file parameter gets a
//! `Dialog::PathPrompt` — a **text box holding a path**, with a `Browse…` button that appears only
//! when a picker is installed in [`Services`]. And those three pickers are *synchronous*:
//!
//! ```ignore
//! pub pick_open:   Option<Box<dyn Fn(&str, &[&str]) -> Option<String>>>,
//! pub pick_save:   Option<Box<dyn Fn(&str, &str) -> Option<String>>>,
//! pub pick_folder: Option<Box<dyn Fn(&str) -> Option<String>>>,
//! ```
//!
//! On the desktop each of those runs `rfd::FileDialog` in line and returns the chosen path, which
//! the dialog then stores in its own text box — the user taps `Browse…`, the box fills in, and they
//! tap `OK`. A host that has to *draw* its own picker (there is no `startActivityForResult` in
//! `android-activity` 0.6, so Android's own Storage Access Framework picker is out of reach) cannot
//! return a path in the same call: the picker needs the frame to end and come back.
//!
//! So the shell splits that one call in two, keeping the app's own dialog — and its `OK`, its
//! extension defaulting and its "already exists — replace?" warning — exactly as they are:
//!
//! 1. `Browse…` is tapped. The closure records *what* was asked (which kind of picker, for which
//!    command) and answers `None`, which is what a cancelled dialog answers: the text box keeps the
//!    value it had, and nothing runs.
//! 2. The shell sees the record on the same frame, opens its own picker with
//!    [`file_picker::request`], and draws it above the app's dialog.
//! 3. When the user chooses, the shell writes the chosen path into the `path` field of the
//!    `Dialog::PathPrompt` that is still open. The user taps `OK` and the command runs, with a real
//!    path, and the session records it.
//!
//! That two-step is not a workaround for SoundCraft: it *is* the desktop's flow, with this picker in
//! place of `rfd`'s. Nothing about the command's semantics is reimplemented here, which is also why
//! there is no upstream patch in this port.
//!
//! # Feature notes
//!
//! - `android-native-activity` **must** be on. Without a backend feature, `android-activity` is
//!   pulled with no backend and the build fails with `file not found for module activity_impl` plus
//!   `Either "game-activity" or "native-activity" must be enabled as features`.
//! - `accesskit` **must** be off, and here it is unreachable: `ui-egui` has no eframe dependency at
//!   all, so this crate is the only eframe user in the graph and never asks for it.
//! - `NativeOptions::android_app` must be `Some(..)`: eframe hands it to
//!   `EventLoopBuilderExt::with_android_app` and returns an error without it.
//!
//! # What Android cannot do yet
//!
//! Honest gaps, not oversights:
//!
//! - **Audio goes through AAudio.** `SoundApp::player` is a real `soundcraft_playback::Player`, which
//!   on Android opens the platform's AAudio output through `cpal`'s Android host (the same call the
//!   desktop makes; `--no-audio` remains the way to run without it). A device whose output cannot be
//!   opened reports it on the status line and plays silently rather than failing — which is also what
//!   the desktop does on a machine with no sound card.
//! - **No plugins.** CLAP, VST3 and AU are loaded from the host's own plugin folders, which do not
//!   exist on the device; a session that references them opens with them missing, as it would on a
//!   desktop without them installed.
//! - **No system menu bar.** `native_menu_bar` is left off (as on Windows and Linux), so the app
//!   keeps its own egui menu bar, fullscreen.
//! - **No drag and drop.** Files cannot be dragged in from a file manager on Android; the picker is
//!   the way in.
//!
//! The session model, the mix engine and the DSP are platform-neutral and unaffected.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use file_picker::Kind;
use soundcraft_engine::Engine;
use soundcraft_ui_egui::UiState;
use soundcraft_ui_egui::dialogs::Dialog;
use soundcraft_ui_egui::{Services, SoundApp};

mod file_picker;
mod modifiers;
mod prefs_watch;

/// The app's private data directory, captured in `android_main` before any service closure runs:
/// `<internal storage>/Android/data/ai.storyteller.soundcraft/files`.
///
/// `internal_data_path()` needs no runtime permission — it is the app's own sandbox — which is why
/// the preferences, the autosave folder, the preset folder and the log land here rather than in
/// shared storage.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// How often the preferences are looked at; a change is written at the next look. Short enough that
/// nothing a user would miss is stale, long enough to cost nothing while the app is idle.
const PREFS_EVERY: Duration = Duration::from_secs(2);

/// What a `Browse…` inside one of the app's own dialogs asked for.
///
/// The synchronous picker in [`Services`] has to answer in the same call, so it cannot open a dialog
/// that is drawn between frames. It records the request here instead and answers `None`, and the
/// shell answers it from the picker's own answer on a later frame (see the module docs).
struct Intent {
    /// Which dialog to show.
    kind: Kind,
    /// The command it was asked for — `session.open`, `file.import_audio`, … — which is what decides
    /// the file types the listing is filtered to.
    purpose: String,
    /// A file name to start a Save dialog with.
    suggested: Option<String>,
}

/// The one `Browse…` waiting to be shown. One slot: a dialog can only have one picker up, and the
/// app asks for the next one after the last is answered.
static INTENT: Mutex<Option<Intent>> = Mutex::new(None);

/// Record a `Browse…` for the shell to answer on this frame.
fn record(kind: Kind, purpose: &str, suggested: Option<String>) {
    if let Ok(mut slot) = INTENT.lock() {
        *slot = Some(Intent { kind, purpose: purpose.to_string(), suggested });
    }
}

/// The file types a pick for `purpose` should offer. SoundCraft's `Browse…` passes no filter list
/// (`pick(cmd, &[])`), so the list is derived from the command, the way its own browser shell does.
fn extensions_for(purpose: &str) -> Option<Vec<String>> {
    const SESSION: &[&str] = &["scraft", "scraftz"];
    const AUDIO: &[&str] = &[
        "wav", "wave", "bwf", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "m4a", "aac", "mp4", "caf", "mkv", "webm",
    ];
    const MIDI: &[&str] = &["mid", "midi", "smf"];
    const VIDEO: &[&str] = &["mp4", "mov", "mkv", "webm", "avi"];
    let filters: &[&[&str]] = match purpose {
        "session.open" | "session.save" | "session.save_as" | "session.save_copy" | "session.save_template"
        | "file.export_selected_tracks_as_session" => &[SESSION],
        "file.import_audio" => &[AUDIO],
        "file.import_midi" => &[MIDI],
        "file.import_video" => &[VIDEO],
        // Anything else — a score to print, a folder to choose — is the user's own business, and the
        // picker lists every file (with "Show all files" always one tap away).
        _ => &[],
    };
    file_picker::extensions(filters)
}

/// The picker's caption, from what the asking command is.
fn pick_title(kind: Kind, purpose: &str) -> String {
    let what = match purpose {
        "session.open" => "Open Session",
        "session.save" | "session.save_as" => "Save Session As",
        "session.save_copy" => "Save Copy In",
        "file.import_audio" => "Import Audio",
        "file.import_midi" => "Import MIDI",
        "file.bounce_mix" => "Bounce Mix",
        "dir" => "Choose Folder",
        _ => match kind {
            Kind::Save => "Save",
            Kind::Folder => "Choose Folder",
            Kind::Open | Kind::OpenMany => "Open",
        },
    };
    format!("{what} — Android")
}

/// The Android [`Services`]: the same three pickers the desktop installs, answering through the
/// shell's own picker instead of `rfd` (see the module docs).
fn android_services() -> Services {
    Services {
        pick_open: Some(Box::new(|purpose: &str, _exts: &[&str]| {
            record(Kind::Open, purpose, None);
            // `None` is what a cancelled dialog answers: the text box keeps what it had, and nothing
            // runs. The shell fills it in a frame later.
            None
        })),
        pick_save: Some(Box::new(|purpose: &str, default: &str| {
            let name = Path::new(default).file_name().map(|n| n.to_string_lossy().into_owned());
            record(Kind::Save, purpose, name);
            None
        })),
        pick_folder: Some(Box::new(|_start: &str| {
            record(Kind::Folder, "dir", None);
            None
        })),
    }
}

/// Write the preferences if they changed, at most every [`PREFS_EVERY`] (see [`prefs_watch`]).
fn save_prefs_if_due(app: &SoundApp, ui: &mut prefs_watch::Writer) {
    if ui.due() {
        let bytes = prefs_bytes(app);
        ui.store(&bytes);
    }
}

/// What goes to `ui.json`: the window's own state, which is what the desktop writes.
fn prefs_bytes(app: &SoundApp) -> Vec<u8> {
    serde_json::to_vec_pretty(&app.ui).unwrap_or_default()
}

/// The preferences file, in the app's own folder, which always exists.
fn prefs_file() -> Option<PathBuf> {
    let dir = DATA_DIR.get()?;
    let _ = std::fs::create_dir_all(dir);
    Some(dir.join("ui.json"))
}

/// Mirrors the desktop's `load_prefs`: `ui.json`, in the app's own folder.
fn load_prefs() -> UiState {
    let Some(dir) = DATA_DIR.get() else {
        return UiState::default();
    };
    match std::fs::read(dir.join("ui.json")) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(ui) => ui,
            Err(e) => {
                log::warn!("using default preferences: the saved ones could not be read ({e})");
                UiState::default()
            }
        },
        Err(_) => UiState::default(),
    }
}

/// A minimal file logger, so a sideloaded APK leaves a readable trace.
///
/// `logcat` is only reachable over adb; a file in the app's own data directory can be pulled with
/// any file manager, which is what a sideloaded build needs.
struct FileLogger {
    sink: Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if let Ok(mut guard) = self.sink.lock()
            && let Some(f) = guard.as_mut()
        {
            use std::io::Write;
            let _ = writeln!(f, "[{}] {} {}", record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

fn init_logging(dir: &Path) {
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("soundcraft.log")) else {
        return;
    };
    let logger = LOGGER.get_or_init(|| FileLogger { sink: Mutex::new(None) });
    if let Ok(mut guard) = logger.sink.lock() {
        *guard = Some(file);
    }
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// The Android entry point.
///
/// `android-activity` spawns a dedicated thread for this and calls it with the activity handle.
/// `#[unsafe(no_mangle)]` with the Rust ABI is the required shape (edition 2024 requires the
/// `unsafe(..)` wrapper around `no_mangle`), and it is the symbol the injected NativeActivity looks
/// for.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    // Count this window while its loop runs (see `prefs_watch`).
    let _window = prefs_watch::counted_window();
    let data_dir = app.internal_data_path().unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = DATA_DIR.set(data_dir.clone());
    init_logging(&data_dir);
    log::info!("SoundCraft {} starting on Android; data dir {}", env!("CARGO_PKG_VERSION"), data_dir.display());
    // The app's external files directory: always readable and writable with no permission, so the
    // picker offers it as a root and Save suggests it first (see `file_picker`).
    if let Some(ext) = app.external_data_path() {
        let _ = std::fs::create_dir_all(&ext);
        let _ = file_picker::EXT_DIR.set(ext.clone());
        // SoundCraft resolves its dialog defaults from `$HOME` (`dialogs::default_dir`), which is
        // unset on Android: without this every path prompt would open on the process's "." and a Save
        // would suggest a file in the root directory. Point it at the folder a user can see. Set here,
        // before the window opens and before any other thread exists.
        unsafe { std::env::set_var("HOME", &ext) };
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("SoundCraft").with_app_id("ai.storyteller.soundcraft"),
        // Required on Android: eframe hands this to the event loop builder and errors without it.
        android_app: Some(app),
        ..Default::default()
    };

    let result = eframe::run_native(
        "SoundCraft",
        options,
        Box::new(|cc| {
            // A real player: `cpal` reaches Android through its AAudio host, so the same call the
            // desktop makes opens the platform's output device. A device with nothing to open reports
            // it rather than failing to start.
            let engine = Engine::default();
            let player = soundcraft_playback::Player::new(Arc::new(engine.session().clone()));
            let mut app = SoundApp::new(engine, Some(player), android_services());
            let prefs = load_prefs();
            app.extra.ui_scale = prefs.ui_scale;
            app.ui = prefs;
            // Autosave and the user's plugin presets: both have to be somewhere the app can always
            // write, which is the same reason the desktop gives them a per-user folder.
            if let Some(dir) = DATA_DIR.get() {
                let (autosave, presets) = (dir.join("Autosave"), dir.join("Presets"));
                let _ = std::fs::create_dir_all(&autosave);
                let _ = std::fs::create_dir_all(&presets);
                app.autosave_dir = Some(autosave);
                app.preset_dir = Some(presets);
            }
            // The egui menu bar, as on Windows and Linux; nothing on Android draws a system one.
            app.native_menu_bar = false;
            if let Some(rs) = &cc.wgpu_render_state {
                log::info!("wgpu adapter: {:?}", rs.adapter.get_info());
            } else {
                log::warn!("no wgpu render state");
            }
            Ok(Box::new(Shell {
                inner: app,
                modifiers: modifiers::Tracked::default(),
                prefs_ui: prefs_watch::Writer::new(prefs_file(), PREFS_EVERY),
                pick: None,
            }))
        }),
    );

    match result {
        Ok(()) => log::info!("SoundCraft exited normally"),
        Err(e) => log::error!("SoundCraft stopped: {e}"),
    }
}

/// Wraps the real app so the platform can act on the frame around it: the picker its own dialogs ask
/// for, and the end of the process when the user quits.
///
/// The same shape as the desktop shell (`apps/soundcraft/src/main.rs`) and the web build:
/// `SoundApp` stays untouched and the platform-specific layer hangs off the outside. Every method is
/// forwarded, so the app sees no difference.
struct Shell {
    inner: SoundApp,
    /// Modifier keys, tracked here because the platform never reports them: winit's Android backend
    /// sends no `ModifiersChanged` at all, which is why Shift+click and every Ctrl shortcut are dead
    /// without this (see `modifiers`).
    modifiers: modifiers::Tracked,
    /// The preferences, written as the app runs rather than only on the way out: Android never gives
    /// that ending, so a swipe from Recents would otherwise take every preference with it.
    prefs_ui: prefs_watch::Writer,
    /// The picker this shell is showing for a `Browse…` one of the app's own dialogs asked for.
    pick: Option<Receiver<Vec<PathBuf>>>,
}

impl Shell {
    /// Start the picker a `Browse…` asked for, and put its answer where `rfd`'s would have gone.
    ///
    /// Called after the app has drawn itself (so a `Browse…` tapped this frame opens its dialog in the
    /// same frame) and before [`file_picker::draw`], which is what actually shows it.
    fn answer_picker(&mut self, ctx: &egui::Context) {
        if let Ok(mut slot) = INTENT.lock()
            && let Some(intent) = slot.take()
        {
            let title = pick_title(intent.kind, &intent.purpose);
            log::info!("{}: showing the picker", intent.purpose);
            self.pick = Some(file_picker::request(intent.kind, &title, extensions_for(&intent.purpose), intent.suggested));
        }
        // Polled rather than waited for on a thread: the answer is wanted on the UI thread, where the
        // app can be given it, and `try_recv` costs nothing when there is none.
        let answer = match self.pick.as_ref() {
            None => return,
            Some(rx) => match rx.try_recv() {
                Ok(paths) => Some(paths),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Vec::new()),
            },
        };
        let Some(paths) = answer else {
            return;
        };
        self.pick = None;
        let Some(path) = paths.into_iter().next() else {
            // Cancelled: the dialog keeps the path it had, exactly as a cancelled `rfd` dialog leaves
            // it.
            log::info!("picker cancelled");
            return;
        };
        match self.inner.dialogs.open.as_mut() {
            Some(Dialog::PathPrompt { path: field, .. }) => {
                log::info!("picker chose {}", path.display());
                *field = path.to_string_lossy().into_owned();
                ctx.request_repaint();
            }
            // The dialog was closed while the picker was up (the user tapped Cancel and then OK, or
            // the command was replaced). Nothing to fill in, and nothing runs.
            _ => log::warn!("the picker answered, but the dialog that asked for it is gone"),
        }
    }

    /// The quit path: end the process.
    ///
    /// `app.quit` sets [`SoundApp::quit_requested`], and the desktop answers it with
    /// `ViewportCommand::Close` and lets the window closing take the process with it. **On Android
    /// that ends nothing**: `egui-winit` turns `ViewportCommand::Close` into a plain
    /// `ViewportEvent::Close` (it never calls winit's window API, which is stubbed on Android
    /// anyway), so the app sits on a stale close request — it looks frozen, dies on the next input
    /// event, and cannot be reopened until it is cleared from Recents, because the native thread
    /// returned while the Activity lived on. So the process ends here.
    ///
    /// Nothing can be unsaved by this point: `quit_requested` is only set once `app.quit` has run,
    /// and the app asks to save first (`dialogs::ask_to_save`). What is left is what `on_exit` would
    /// have done — save the preferences — and it is done here because a process exit runs no
    /// `on_exit`.
    fn quit_if_requested(&mut self) {
        if !self.inner.quit_requested {
            return;
        }
        let bytes = prefs_bytes(&self.inner);
        self.prefs_ui.store_now(&bytes);
        log::info!("File ▸ Quit: ending the process");
        // A process exit runs no `on_exit`, so an unflushed file logger loses the last lines.
        log::logger().flush();
        soundcraft_engine::shutdown_plugin_hosts();
        std::process::exit(0);
    }
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Before the app, so a `Browse…` tapped in the previous frame's `ui` has its picker up by the
        // time the dialog is drawn again.
        self.answer_picker(ctx);
        self.inner.logic(ctx);
        save_prefs_if_due(&self.inner, &mut self.prefs_ui);
        self.quit_if_requested();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Before the app, so the frame it sees already has the modifiers a desktop backend would have
        // reported (see `modifiers`).
        self.modifiers.apply(raw);
        self.inner.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.inner.ui(ui);
        // After the app, so a `Browse…` tapped inside its own dialog opens this window's picker in the
        // same frame and so the picker floats above the dialog.
        self.answer_picker(ui.ctx());
        file_picker::draw(ui.ctx());
        // The quit dialog's OK is a click during this call: check here, in the same frame, rather than
        // leaving it to the next `logic` — no repaint is guaranteed after a click.
        self.quit_if_requested();
    }

    fn on_exit(&mut self) {
        let bytes = prefs_bytes(&self.inner);
        self.prefs_ui.store_now(&bytes);
    }
}
