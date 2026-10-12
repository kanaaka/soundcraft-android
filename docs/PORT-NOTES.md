# Port notes — SoundCraft on Android

Upstream: [`storytold/soundcraft`](https://github.com/storytold/soundcraft) @
`acb08ff5f00d654a5303194ad04dad799c68a9fb` (v0.5.0)
Published APK: `soundcraft-0.5.0-arm64-v8a.apk`

The port is `apps/soundcraft-android/` (`src/lib.rs`, `src/file_picker.rs` 403,
`src/modifiers.rs` 92, `src/prefs_watch.rs` 134) plus **one upstream file** —
`crates/ui-egui/src/fonts.rs`, which gains the Android candidates for the system UI font (§7). This
is the smallest patch in the set, and the reason is §2: SoundCraft already has the seam the port
needs.

## 1. The shell crate

`soundcraft_ui_egui::Services` is a struct of three `Option<Box<dyn Fn…>>` fields that derives
`Default`; the desktop binary fills it with `rfd`, the browser shell leaves it empty, and Android is
the third. Load-bearing in every port of this set:

- **`[lib] name = "soundcraft"` + `crate-type = ["cdylib"]`** → `lib/arm64-v8a/libsoundcraft.so`, with
  the injected `android.app.lib_name` pointing at that artifact name (`-` → `_`). A mismatch installs
  and then dies on launch.
- **`options.android_app = Some(app)`** — `eframe::run_native` errors without it.
- **A file logger in the app's own data directory** — `logcat` needs adb, and a sideloaded build has
  none.
- **Preferences under `internal_data_path()`**, written while the app runs (§3).
- **The autosave and preset folders** the desktop sets from its settings directory
  (`app.autosave_dir`, `app.preset_dir`) go under the same data directory.

`cargo check -p soundcraft-ui-egui --target aarch64-linux-android` passes with **no changes at all**:
`ui-egui` has no `rfd`, no `arboard`, and no `eframe` dependency of its own (the desktop shell owns
them), so the whole of it — the Edit and Mix windows, the transport, the meters, the score editor, the
dialogs — is platform-neutral already.

## 2. The file dialogs: a *synchronous* seam, split in two

This is the one thing this port does differently from its siblings, and it is worth reading before
porting another app of this suite.

SoundCraft's dialogs ask for a path in two parts. Almost every command with a file parameter gets a
`Dialog::PathPrompt` — **a text box holding a path**, plus a `Browse…` button that appears only when a
picker is installed:

```rust
pub pick_open:   Option<Box<dyn Fn(&str, &[&str]) -> Option<String>>>,   // Browse… for a file
pub pick_save:   Option<Box<dyn Fn(&str, &str) -> Option<String>>>,      // Browse… for a save
pub pick_folder: Option<Box<dyn Fn(&str) -> Option<String>>>,            // Browse… for a folder
```

Those three are **synchronous**: the desktop runs `rfd::FileDialog` in line, returns the path, and the
dialog stores it in its own text box; the user then taps `OK`, which runs the command. A host that
draws its own picker cannot do that in one call — the picker needs the frame to end and come back (and
Android's own Storage Access Framework picker is out of reach anyway, because it needs
`startActivityForResult`, which `android-activity` 0.6 does not expose).

So the shell splits that call in two and keeps everything else — the app's own dialog, its `OK`, its
extension defaulting, and its *"already exists — replace?"* warning — exactly as it is:

1. `Browse…` is tapped. The closure records what was asked (which kind of picker, for which command)
   in a `static`, and answers `None` — which is what a cancelled dialog answers, so the text box keeps
   the value it had and nothing runs.
2. The shell sees the record on the same frame (it checks after forwarding to the app's `ui`),
   opens its own picker with `file_picker::request`, and draws it above the dialog.
3. When the user chooses, the shell writes the chosen path into the `path` field of the
   `Dialog::PathPrompt` that is still open and requests a repaint. The user taps `OK`; the command runs
   with a real path and the session records it.

That two-step is the desktop's own flow with this picker in place of `rfd`'s, not a workaround: the
desktop's `Browse…` also fills the box and leaves `OK` to the user. Nothing about a command's
semantics is reimplemented in the shell, which is why this port needs no upstream patch for it.

The picker drawing rules are the ones the whole set shares: sized from `ctx.content_rect()`, enlarged
padding for fingers, a repaint requested while open, listings and readability probes cached per visit
rather than per frame, locked roots left tappable so they can name the permission and the Settings path
they need, the filter shown as the union of every extension asked for with **"Show all files"** and a
count of the hidden ones, and a save answer of `dir.join(name)` so the app's atomic write finds its
parent. The extension lists come from the command id (`session.open` → `scraft`/`scraftz`,
`file.import_audio` → the audio list, `file.import_midi` → `mid`/`midi`/`smf`), because SoundCraft's
`Browse…` passes no filter list — the same derivation its own browser shell makes.

`$HOME` is pointed at the app's external files directory in `android_main`, because
`dialogs::default_dir()` resolves a path prompt's starting folder from it and Android leaves it unset:
without that, every prompt would open on the process's `.` and a Save would suggest a file in the root
directory.

## 3. Preferences are written while the app runs

The desktop writes `ui.json` on its way out (`config::save_ui`); Android gives no such moment, so a
swipe from Recents took every preference with it — the docking layout, the Edit/Mix window choice,
the interface scale, the visible panels. `prefs_watch` writes `ui.json` from the frame loop:
change-detected, at most every two seconds, atomically (temp file + rename, so a kill mid-write cannot
leave truncated JSON where the preferences were).

## 4. Ctrl, Shift and Alt did nothing — `src/modifiers.rs`

winit's Android backend emits only six window events and never `ModifiersChanged`, and `egui-winit`
updates modifiers from that event and nowhere else; egui carries modifiers as an *event*, so
`ctx.input(|i| i.modifiers)` is the all-false default for the entire run — Ctrl+S, Ctrl+Z and
Shift-click are dead. The key events do arrive, so the shell tracks the modifiers in `raw_input_hook`
and re-injects them the way egui expects: folded into the held state, stamped onto every `Key` and
`PointerButton` event, plus one `ModifiersChanged` per frame. State is carried **between** frames,
because the press and the click usually land in different ones.

## 5. The Activity is not recreated (`config_changes`)

`[package.metadata.android.application.activity] config_changes = "orientation|keyboardHidden|…"` so
a rotation, the keyboard appearing, a density or font-scale change, or a split-screen resize does not
recreate the Activity. A recreated Activity destroys the native window while the render thread lives
on — a blank surface.

## 6. `File ▸ Quit` ends the process

The desktop answers `app.quit` with `ViewportCommand::Close` and lets the window closing take the
process with it. **On Android that ends nothing**: `egui-winit` turns `ViewportCommand::Close` into a
plain `ViewportEvent::Close` (it never calls winit's window API, which is stubbed on Android anyway),
so the app sits on a stale close request — it looks frozen, dies on the next input event, and then
cannot be reopened until it is cleared from Recents, because the native thread returned while the
Activity lived on. The shell ends the process instead, after `log::logger().flush()` (a process exit
runs no `on_exit`, so an unflushed file logger loses its last lines) and after writing the preferences.

Nothing can be unsaved by that point: the menu's Quit runs through `dialogs::run_or_ask`, so the
app has already asked to save. The flag is checked in **both** `logic` and `ui`, because the menu click
that sets it happens during `ui` and no repaint is guaranteed afterwards.

## 7. System fonts come from `/system/fonts` — the one upstream change

`crates/ui-egui/src/fonts.rs` loads the operating system's UI font at runtime from a list of
candidate paths, and that list was macOS, Windows and Linux only: on Android every candidate missed and
the app fell back to egui's bundled defaults, which are not built for a dense DAW layout. The list
gains the platform's own faces first — `/system/fonts/Roboto-Regular.ttf`, `RobotoStatic-Regular.ttf`
and `NotoSans-Regular.ttf` for text, with `Roboto-Bold.ttf`, `RobotoStatic-Bold.ttf` and
`NotoSans-Bold.ttf` for the bold family (a device without a bold face falls back to the regular one,
which the caller already handles). This is additive: the desktop's own candidates are untouched and
its order of preference is unchanged.

## 8. Audio runs through AAudio

`SoundApp::player` is a real `soundcraft_playback::Player`, constructed exactly as the desktop
constructs it. `cpal` reaches Android through its **AAudio** host on this toolchain, so the platform's
own output device is opened. A device whose output cannot be opened reports it and plays silently —
which is what the desktop does on a machine with no sound card, and what `--no-audio` asks for.

## Upstream files this port changes (1)

| File | Change | Why |
|---|---|---|
| `crates/ui-egui/src/fonts.rs` | Android candidates added to `REGULAR` and `BOLD` | no candidate matched on Android, so the UI fell back to egui's bundled defaults (§7) |

## What is deliberately *not* in the patch

`Cargo.lock` is excluded on purpose: it moves only because the port crate is a new workspace member,
and `cargo` regenerates it on the first build. The patch contains only changes that matter to a
reader — every tracked file the port touches, plus any new file that has to live inside an upstream
directory. (Here that is one file; the port crate lives in `apps/`, which the workspace's `apps/*`
glob picks up on its own.)

## What Android still cannot do

- **No plugins.** CLAP, VST3 and AU are loaded from the host's own plugin folders, which do not exist
  on the device; a session that references them opens with them missing, as it would on a desktop
  without them installed.
- **No system menu bar.** `native_menu_bar` stays off, as on Windows and Linux, so the app keeps its
  own egui menu bar, fullscreen.
- **No drag and drop.** Files cannot be dragged in from a file manager on Android; the picker is the
  way in.

## Not verified here

The behaviour above was reproduced in an x86_64 tablet emulator (§ the README's verification table);
the arm64 APK in Releases was built from the same sources and was not re-run on a physical device
while publishing this repository. There is no automated device test here, so "verified" in the README
means the source-side checks listed there, the emulator run, and the porter's hand-testing on an arm64
tablet.
