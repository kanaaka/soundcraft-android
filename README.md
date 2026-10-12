# SoundCraft for Android (arm64-v8a)

An unofficial Android port of **[SoundCraft](https://github.com/storytold/soundcraft)** — the real
engine and egui interface, packaged as a NativeActivity APK for sideloading on arm64 devices.

| | |
|---|---|
| Upstream project | [`storytold/soundcraft`](https://github.com/storytold/soundcraft) — Apache-2.0 OR MIT |
| Pinned commit | `acb08ff5f` (upstream version `0.5.0`) |
| Android package | `ai.storyteller.soundcraft` |
| Published APK | `soundcraft-0.5.0-arm64-v8a.apk` — 10.2 MB, arm64-v8a, signed |
| APK sha256 | `fccb35698ea1d25f3cd2c6705f7f088f4cc70597869aab7e68fb6e78eb0ba39e` |
| Upstream files changed | 1 |
| New port crate | `apps/soundcraft-android/` — about 1 200 lines of Rust plus resources |

## What this repository is

This is **not a fork** of SoundCraft. It holds only the Android layer, so it stays small, readable,
and obviously separable from upstream:

```
apps/soundcraft-android/   the new shell crate — copy it into your checkout of upstream
patches/                   the diff against upstream — apply it after checking out the pin
docs/PORT-NOTES.md         what was changed and why, fix by fix, with how each one was checked
scripts/                   cross-compile environment, build and verify scripts (from shared-android-port)
CHANGELOG.md               what is in the published APK
```

The Android layer is not a reimplementation. It is a shell crate that fills the same `Services`
struct the desktop and the browser shells fill, so the real engine and the real egui UI run unchanged.

## Build it

Prerequisites: Rust (rustup) with the `aarch64-linux-android` target, Android SDK + NDK r27, a
JDK 21, and `cargo install cargo-apk`.

```bash
git clone https://github.com/storytold/soundcraft.git
cd soundcraft
git fetch --depth 1 origin acb08ff5f00d654a5303194ad04dad799c68a9fb
git checkout FETCH_HEAD                        # the exact commit this port targets
git apply /path/to/0001-soundcraft-android-port.patch
cp -r /path/to/soundcraft-android apps/        # the shell crate from this repository

# any keystore of your own signs an installable APK; a throwaway test key is fine for sideloading
keytool -genkeypair -keystore ks.jks -alias soundcraft -keyalg RSA -keysize 2048 -validity 10000 \
        -storepass android -keypass android -dname "CN=Local Test, O=Local, C=US"
export CARGO_APK_RELEASE_KEYSTORE="$PWD/ks.jks" CARGO_APK_RELEASE_KEYSTORE_PASSWORD=android
cargo apk build --release -p soundcraft-android
```

`scripts/android-env.sh` is the cross-compile environment this port was built with (the NDK linker
and the C compiler the native dependencies need, and the x86_64 linker for an emulator build),
`scripts/build-apk.sh` wraps the build, and `scripts/verify-apk.sh` checks the artifact:
`aapt2 dump badging`, `apksigner verify`, and `llvm-nm` on the `.so` inside the APK.

## Install

```bash
adb install -r soundcraft-0.5.0-arm64-v8a.apk
```

or copy the APK to the device and open it (installing from unknown sources must be allowed).

On first run, grant **Settings ▸ Apps ▸ SoundCraft ▸ Special access ▸ All files access** if you want
the in-app picker to reach files outside the app's own folders. Without it, Android 10+ refuses a
`read_dir` of shared storage by path, and the picker can only offer the app's own directory.

## Verification status

Everything below was reproduced on this build unless the row says otherwise. "Emulator" means an
x86_64 **Android 15 (API 35)** Pixel Tablet AVD; the arm64 APK is built from the same sources.

| Claim | How it was checked |
|---|---|
| Installs and launches | `adb install -r`, then `am start -n ai.storyteller.soundcraft/android.app.NativeActivity`; the app's own log (`/data/data/ai.storyteller.soundcraft/files/soundcraft.log`) starts with `SoundCraft 0.5.0 starting on Android` |
| The real UI runs | a screen grab of the Edit window: menu bar, Shuffle/Spot/Slip/Grid, transport, big counter, `Tempo 120` / `Meter 4/4` rulers, Tracks and Groups pane, Clips pane, status line `Untitled · 48000 Hz · 24-bit · 0 tracks` |
| Text is legible, not egui's fallback font | the `fonts.rs` change loads `/system/fonts`; the rulers and the numeric counter render in the platform face |
| **Audio works** | the log reports `soundcraft_playback audio device: default @ 48000 Hz, 2 ch` — `cpal`'s AAudio host opened the emulator's output device, and the transport strip shows `audio on` |
| **The file picker works, end to end** | File ▸ *Save Session Copy In* → `Browse…` → the in-app picker → *Save* → `OK`; the log shows `picker chose /storage/emulated/0/Android/data/ai.storyteller.soundcraft/files/Untitled.scraft`, and that exact path — not the dialog's own default — exists on disk afterwards |
| Preferences survive | `ui.json` (5 KB, the whole docking layout) is written into the app's private data directory **while the app runs**, so a swipe from Recents cannot lose it |
| **Quit ends the process** | the app menu ▸ *Quit*: the log ends with `File ▸ Quit: ending the process`, `pidof` is empty, the task is gone from `dumpsys activity activities`, and the app relaunches cleanly |
| Modifier keys | five unit tests in `src/modifiers.rs`: a click in a *later* frame still carries the Shift held in an earlier one, Ctrl+S reports `command`, a modifier going up stops being held, and a platform report overrides the tracked state |
| The artifact is what it claims | `aapt2 dump badging` → package `ai.storyteller.soundcraft`, `versionName 0.5.0`, `minSdk 26`, `targetSdk 35`, `native-code: 'arm64-v8a'`; `apksigner verify --verbose` → v2 and v3 true; `llvm-nm --dynamic --defined-only` on the `.so` inside the APK → `android_main` and `ANativeActivity_onCreate`; the injected `android.app.lib_name` is `soundcraft`, matching `libsoundcraft.so`; the theme resolves to `attr/windowFullscreen` |

**Not verified:** the modifier keys could not be exercised in the emulator (`adb` cannot deliver a
*held* modifier — `input keyevent` sends a down/up pair), so they are unit-tested rather than
device-tested; the arm64 APK in Releases was not re-run on a physical device while publishing this
repository.

## Known gaps

- **No plugins.** CLAP, VST3 and AU are loaded from the host's own plugin folders, which do not exist
  on the device: a session that references them opens with them missing, as it would on a desktop
  without them installed.
- **No system menu bar,** as on Windows and Linux, so the app keeps its own egui menu bar.
- **No drag and drop** — files cannot be dragged in from a file manager on Android; the picker is the
  way in.
- **The navigation bar stays.** The theme hides the status bar (which otherwise covers the menu bar,
  where it cannot be tapped); hiding the navigation bar too needs the Java immersive-mode API.
- Anything about GPU-specific rendering, touch/IME nuance and performance is not something an
  emulator running SwiftShader can answer.

## The rest of this set

- [DeckCraft](https://github.com/kanaaka/deckcraft-android)
- [effectcraft](https://github.com/kanaaka/effectcraft-android)
- [FilmCraft](https://github.com/kanaaka/filmcraft-android)
- [GridCraft](https://github.com/kanaaka/gridcraft-android)
- [LightCraft](https://github.com/kanaaka/lightcraft-android)
- [PdfCraft](https://github.com/kanaaka/pdfcraft-android)
- [PhotoCraft](https://github.com/kanaaka/photocraft-android)
- [VectorCraft](https://github.com/kanaaka/vectorcraft-android)
- [WordCraft](https://github.com/kanaaka/wordcraft-android)
- [shared-android-port](https://github.com/kanaaka/shared-android-port) — the pieces every port shares

## Credits and licence

SoundCraft is © its authors at [`storytold/soundcraft`](https://github.com/storytold/soundcraft) and
is dual-licensed Apache-2.0 / MIT; both licence texts are kept here. The Android layer in this
repository is an unofficial modification by [@kanaaka](https://github.com/kanaaka), carrying the same
licence. Not affiliated with or endorsed by the upstream project. See
[`ATTRIBUTION.md`](ATTRIBUTION.md).
